package pl.lokalsi.localllm

import android.app.Activity
import android.app.ActivityManager
import android.content.Context
import android.os.Debug
import android.os.SystemClock
import app.tauri.Logger
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.google.ai.edge.litertlm.Backend
import com.google.ai.edge.litertlm.Conversation
import com.google.ai.edge.litertlm.ConversationConfig
import com.google.ai.edge.litertlm.Contents
import com.google.ai.edge.litertlm.Engine
import com.google.ai.edge.litertlm.EngineConfig
import com.google.ai.edge.litertlm.Message
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.coroutines.launch

@InvokeArg
class LoadArgs {
    lateinit var path: String

    /** "cpu" (default) or "gpu" */
    var backend: String = "cpu"
}

@InvokeArg
class HistoryItem {
    /** "user" or "assistant" */
    lateinit var role: String
    lateinit var content: String
}

@InvokeArg
class GenerateArgs {
    lateinit var prompt: String
    var system: String? = null

    /** Earlier turns of this chat; the plugin keeps no conversation between calls. */
    var history: Array<HistoryItem> = emptyArray()
    lateinit var onEvent: Channel
}

/**
 * On-phone chat model (spike, docs/plans/android-local-chat.md).
 * Events on the channel: {"token": "..."} … then {"done": true, ...numbers},
 * {"cancelled": true} or {"error": "..."}.
 * Prompt and answer texts are never written to the log.
 */
@TauriPlugin
class LocalLlmPlugin(private val activity: Activity) : Plugin(activity) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    @Volatile private var engine: Engine? = null
    @Volatile private var job: Job? = null
    private var backendName = "cpu"

    /** Load, unload and destroy run one at a time, so one never closes an engine another just made. */
    private val lifecycle = Mutex()

    /**
     * Stops a running answer and only then closes the engine: cancelling the coroutine
     * alone does not stop the native call, and closing the engine under it can crash.
     */
    private suspend fun stopAndClose() {
        val running = job
        job = null
        stopNative()
        // The native call ends on cancelProcess; the limit only keeps a stuck one from blocking load forever.
        withTimeoutOrNull(8000) { running?.cancelAndJoin() }
        val old = engine
        engine = null
        old?.close()
    }

    /** The conversation that is answering now; cancelling the coroutine alone does not stop it. */
    @Volatile private var active: Conversation? = null

    private fun stopNative() {
        runCatching { active?.cancelProcess() }
    }

    private fun pssMb(): Long {
        val dbg = Debug.MemoryInfo()
        Debug.getMemoryInfo(dbg)
        return dbg.totalPss / 1024L
    }

    private fun memory(): JSObject {
        val am = activity.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
        val info = ActivityManager.MemoryInfo()
        am.getMemoryInfo(info)
        return JSObject().apply {
            put("totalMb", info.totalMem / 1048576)
            put("availMb", info.availMem / 1048576)
            put("lowMemory", info.lowMemory)
            put("pssMb", pssMb())
        }
    }

    @Command
    fun status(invoke: Invoke) {
        invoke.resolve(JSObject().apply {
            put("loaded", engine != null)
            put("backend", backendName)
            put("memory", memory())
        })
    }

    @Command
    fun load(invoke: Invoke) {
        val args = invoke.parseArgs(LoadArgs::class.java)
        scope.launch {
            lifecycle.withLock {
            try {
                stopAndClose()
                val started = SystemClock.elapsedRealtime()
                val backend = if (args.backend == "gpu") Backend.GPU() else Backend.CPU()
                val config = EngineConfig(
                    modelPath = args.path,
                    backend = backend,
                    cacheDir = activity.cacheDir.path,
                )
                val e = Engine(config)
                e.initialize()
                engine = e
                backendName = args.backend
                val ms = SystemClock.elapsedRealtime() - started
                Logger.info("localllm: loaded in $ms ms, backend ${args.backend}")
                invoke.resolve(JSObject().apply {
                    put("loadMs", ms)
                    put("memory", memory())
                })
            } catch (t: Throwable) {
                Logger.error("localllm: load failed: ${t.javaClass.simpleName}")
                invoke.reject(t.message ?: t.javaClass.simpleName)
            }
            }
        }
    }

    @Command
    fun generate(invoke: Invoke) {
        val args = invoke.parseArgs(GenerateArgs::class.java)
        val e = engine
        if (e == null) {
            invoke.reject("model_not_loaded")
            return
        }
        // A new answer starts only after the previous one has really stopped.
        val previous = job
        previous?.cancel()
        job = scope.launch {
            val channel = args.onEvent
            var chunks = 0
            var chars = 0
            val started = SystemClock.elapsedRealtime()
            var first = 0L
            var peakPss = 0L
            try {
                previous?.cancelAndJoin()
                val past = args.history.map {
                    if (it.role == "assistant") Message.model(it.content) else Message.user(it.content)
                }
                val system = args.system?.let { Contents.of(it) }
                val conversation = if (system != null || past.isNotEmpty()) {
                    e.createConversation(ConversationConfig(systemInstruction = system, initialMessages = past))
                } else {
                    e.createConversation()
                }
                conversation.use { c ->
                    active = c
                    c.sendMessageAsync(args.prompt).collect { message ->
                        // A model that reasons first sends its thoughts on a separate channel.
                        if (message.channels["thought"].isNullOrEmpty().not()) {
                            channel.send(JSObject().apply { put("thought", true) })
                        }
                        val text = message.toString()
                        if (text.isNotEmpty()) {
                            if (first == 0L) first = SystemClock.elapsedRealtime() - started
                            chunks += 1
                            chars += text.length
                            if (chunks % 16 == 0) peakPss = maxOf(peakPss, pssMb())
                            channel.send(JSObject().apply { put("token", text) })
                        }
                    }
                }
                val total = SystemClock.elapsedRealtime() - started
                Logger.info("localllm: generated $chunks chunks in $total ms")
                channel.send(JSObject().apply {
                    put("done", true)
                    put("chunks", chunks)
                    put("chars", chars)
                    put("firstMs", first)
                    put("totalMs", total)
                    put("peakPssMb", maxOf(peakPss, pssMb()))
                    put("memory", memory())
                })
            } catch (t: CancellationException) {
                channel.send(JSObject().apply { put("cancelled", true) })
            } catch (t: Throwable) {
                Logger.error("localllm: generate failed: ${t.javaClass.simpleName}")
                channel.send(JSObject().apply { put("error", t.message ?: t.javaClass.simpleName) })
            } finally {
                active = null
            }
        }
        invoke.resolve()
    }

    @Command
    fun cancel(invoke: Invoke) {
        stopNative()
        job?.cancel()
        invoke.resolve()
    }

    @Command
    fun unload(invoke: Invoke) {
        scope.launch {
            lifecycle.withLock { stopAndClose() }
            invoke.resolve()
        }
    }

    override fun onDestroy() {
        // Do not hang the main thread for long if the native side is slow to stop.
        runBlocking { withTimeoutOrNull(3000) { lifecycle.withLock { stopAndClose() } } }
        scope.cancel()
    }
}
