# PRD: Lokalomat AI Studio – lokalne studio AI

> Dokument wymagań produktowych do przekazania sesji Claude Code.
> Wersja: 0.1 · Status: szkic do realizacji · Język UI: polski (angielski jako drugi)

---

## 0. Instrukcje dla Claude Code

1. Pracuj etapami zgodnie z sekcją **13. Kamienie milowe**. Nie przechodź do kolejnego etapu, dopóki kryteria akceptacji bieżącego nie są spełnione.
2. **Weryfikuj aktualne wersje** bibliotek, silników i modeli (llama.cpp, whisper.cpp, stable-diffusion.cpp, Tauri, modele TTS) przed użyciem. Nazwy modeli w tym dokumencie są przykładowe.
3. **Nigdy nie wymyślaj sum sha256, rozmiarów plików ani adresów URL wag.** Pobieraj je z API Hugging Face (lub zostaw pole puste z TODO).
4. Każdą decyzję architektoniczną, która odbiega od tego PRD, zapisz w `docs/adr/` (krótki ADR).
5. Układ interfejsu ma być **autorski**. Nie wzoruj się wizualnie na istniejących narzędziach do lokalnego AI. Trzymaj się opisu z sekcji 9.
6. Identyfikatory w kodzie, nazwy plików i commity – po angielsku. Teksty UI – przez system i18n (PL domyślnie).

---

## 1. Wizja i cel

Jedna aplikacja desktopowa (a docelowo także mobilna), która pozwala zwykłemu użytkownikowi uruchamiać lokalnie, na własnym sprzęcie, różne rodzaje modeli AI:

- **czat z modelem językowym (LLM)**,
- **generowanie obrazów**,
- **transkrypcję mowy** (tekst, napisy SRT),
- **syntezę mowy i klonowanie głosu** (w tym po polsku),
- **łańcuchy (pipelines)** łączące powyższe, np. nagranie › transkrypcja › streszczenie przez LLM › lektor sklonowanym głosem.

Zasady nadrzędne:

- **Local-first i prywatność**: bez konta, bez chmury, nic nie opuszcza urządzenia (poza pobieraniem modeli).
- **Prostota**: formularze i tryby zamiast grafów węzłów. Parametry zaawansowane schowane, ale dostępne.
- **Uczciwość wobec sprzętu**: aplikacja mówi wprost, co zmieści się na danym komputerze.
- **Przejrzystość licencji**: przy każdym modelu widać, czy wolno go używać komercyjnie i czy licencja obowiązuje w UE.

### Grupa docelowa

- Twórcy treści, małe firmy, entuzjaści, którzy nie chcą konfigurować Pythona i ComfyUI.
- Użytkownicy zaawansowani, którzy chcą wygodnego panelu do własnych modeli GGUF.

---

## 2. Platformy

| Platforma | Zakres MVP | Akceleracja |
|---|---|---|
| Windows 10/11 x64 | pełny | CUDA (NVIDIA), Vulkan (AMD/Intel), CPU |
| macOS 13+ (Apple Silicon) | pełny | Metal (pamięć zunifikowana) |
| macOS Intel | best effort, CPU | CPU |
| Linux x64 | nieoficjalnie (łatwo, bo Tauri) | CUDA / Vulkan / CPU |
| Android 10+ | **tryb klienta** (faza 1), lokalne małe modele (faza 2) | CPU / Vulkan / NNAPI – do weryfikacji |

---

## 3. Zakres

### 3.1 MVP (wersja 1.0)

- Kreator pierwszego uruchomienia z diagnostyką sprzętu.
- Katalog modeli z filtrami i oceną dopasowania do sprzętu.
- Menedżer pobrań (kolejka, wznawianie, sha256).
- Menedżer silników (instalacja, aktualizacja, stan, logi).
- Tryby pracy: **Czat**, **Obraz**, **Transkrypcja**, **Głos (TTS + klonowanie)**.
- Biblioteka głosów (profile z próbkami referencyjnymi).
- Wspólna historia/galeria wyników ze wszystkich trybów.
- Globalna kolejka zadań z postępem i anulowaniem.
- Łańcuchy: edytor liniowy (lista kroków), co najmniej 3 gotowe szablony.
- Ustawienia, i18n PL/EN, motyw jasny/ciemny.

### 3.2 Faza 2

- Android: lokalne małe LLM (1–4B, GGUF Q4) i Whisper small/base.
- Edycja obrazu (img2img, inpainting), LoRA.
- Separacja wokalu/podkładu, rozróżnianie mówców w transkrypcji.
- RAG na lokalnych dokumentach w czacie.

### 3.3 Faza 3 / poza zakresem MVP

- Generowanie wideo i muzyki.
- Konwersja barwy głosu (RVC-podobne).
- Rynek wtyczek/silników firm trzecich.

---

## 4. Architektura

### 4.1 Warstwy

```
-¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¬
- UI (WebView): TypeScript + Svelte 5 (rekomendacja)    -
-  - widoki, stan, i18n, motywy                         -
L¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦^¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦-
                - Tauri IPC (commands + events)
-¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦+¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¬
- Core (Rust, w powłoce Tauri 2)                        -
-  - hardware probe       - download manager            -
-  - model registry       - engine supervisor           -
-  - job queue + VRAM scheduler   - pipelines runner    -
-  - storage (SQLite)     - local API gateway           -
L¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦^¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦-
                - HTTP na 127.0.0.1 (losowy port + token)
-¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦+¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¬
- Silniki (procesy podrzędne / sidecars)                -
-  - llama.cpp (llama-server)            › LLM          -
-  - whisper.cpp (server)                › STT          -
-  - stable-diffusion.cpp                › obraz        -
-  - Python worker (uv + venv, FastAPI)  › TTS/klon.    -
L¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦-
```

### 4.2 Decyzje

- **Powłoka: Tauri 2.** Lekka, natywna, wspiera Windows, macOS, Linux i Androida.
- **Rdzeń w Ruście** zarządza procesami, pobieraniem i kolejką. UI nigdy nie rozmawia z silnikami bezpośrednio.
- **Silniki natywne z rodziny ggml** jako domyślne (przenośne, lekkie, bez PyTorcha).
- **Python tylko tam, gdzie konieczny** (modele TTS z klonowaniem dostępne wyłącznie w PyTorch). Każdy silnik pythonowy w osobnym środowisku tworzonym przez `uv`, instalowanym na żądanie. Na macOS – PyTorch z MPS; jeśli dostępny wariant MLX, rozważyć go.
- **Wspólny kontrakt silnika** (sekcja 4.3), dzięki któremu dodanie nowego silnika nie wymaga zmian w UI.
- **Lokalna brama API** w rdzeniu udostępnia endpointy zgodne z OpenAI (`/v1/chat/completions`, `/v1/audio/transcriptions`, `/v1/audio/speech`, `/v1/images/generations`). Domyślnie tylko 127.0.0.1. Udostępnienie w LAN (dla Androida) – opcja włączana ręcznie, z parowaniem i tokenem.

### 4.3 Kontrakt silnika (Engine Adapter)

Każdy silnik opisuje plik `engine.json` + adapter w Ruście:

```jsonc
{
  "id": "llama-cpp",
  "name": "llama.cpp",
  "capabilities": ["chat", "embeddings"],
  "kind": "native",            // native | python
  "platforms": ["windows-x64", "macos-arm64", "linux-x64", "android-arm64"],
  "backends": ["cuda", "vulkan", "metal", "cpu"],
  "install": { "source": "github-release", "repo": "ggml-org/llama.cpp", "asset_pattern": "..." },
  "launch": { "cmd": "llama-server", "args": ["--port", "{port}", "-m", "{model_path}"] },
  "health": { "path": "/health" }
}
```

Adapter implementuje: `install()`, `start(model, params)`, `stop()`, `health()`, `run(job) -> stream<Progress|Partial|Result>`, `estimate_memory(model, params)`.

### 4.4 Harmonogram zasobów (VRAM scheduler)

- Rdzeń zna budżet pamięci (VRAM lub pamięć zunifikowana minus zapas systemowy).
- Przed startem zadania sprawdza, czy model się zmieści razem z aktualnie załadowanymi.
- Polityka: LRU – zwalnia najdawniej używany model. Opcja „trzymaj LLM w pamięci”.
- Zadania ciężkie (obraz) i lekkie (transkrypcja na CPU) mogą iść równolegle, jeśli budżet pozwala.
- Wskaźnik zajętości pamięci widoczny stale w nagłówku UI.

### 4.5 Przechowywanie danych

- **SQLite** (`app.db`): modele, pobrania, zadania, wyniki, rozmowy, głosy, łańcuchy, ustawienia.
- **Pliki**: `<data_dir>/models/`, `<data_dir>/engines/`, `<data_dir>/outputs/YYYY-MM/`, `<data_dir>/voices/`.
- Lokalizacja katalogu danych wybierana w kreatorze (modele bywają duże – wybór dysku).
- Każdy wynik ma metadane: model, silnik, parametry, seed, czas generowania, źródło (tryb lub łańcuch).

---

## 5. Diagnostyka sprzętu

Wykrywane:

- GPU: producent, model, VRAM, wersja sterownika (NVIDIA: NVML lub `nvidia-smi`; Apple: system_profiler/Metal; AMD/Intel: Vulkan).
- Na Apple Silicon: pamięć zunifikowana – budżet = RAM × współczynnik (domyślnie 0.7, konfigurowalny).
- RAM, rdzenie CPU, obsługa AVX2/AVX-512 (x64) lub NEON (ARM).
- Wolne miejsce na dysku wybranym na dane.

Wynik: profil sprzętowy zapisany w bazie, odświeżany na żądanie.

### Ocena dopasowania modelu

Dla każdego wariantu modelu: `estimated_memory` vs budżet ›

- **Pasuje** – zapas ? 15%,
- **Na granicy** – zapas 0–15% (ostrzeżenie, możliwe spowolnienie),
- **Z odciążeniem** – nie mieści się w VRAM, ale zmieści się z offloadem do RAM (wolniej),
- **Nie zadziała** – za mało pamięci łącznie.

Etykiety mają być zrozumiałe dla laika, z tooltipem pokazującym liczby.

---

## 6. Katalog modeli

### 6.1 Manifest

Katalog to plik `catalog/models.json` w repozytorium (później opcjonalnie aktualizowany z serwera, z podpisem). Schemat:

```jsonc
{
  "id": "whisper-large-v3-turbo",
  "name": "Whisper large-v3-turbo",
  "publisher": "OpenAI",
  "modality": "stt",              // llm | image | stt | tts
  "engine": "whisper-cpp",
  "description_pl": "…",
  "description_en": "…",
  "languages": ["pl", "en", "…"],
  "features": ["timestamps", "srt"],   // np. voice_cloning, voice_design, polish_tts
  "license": {
    "name": "MIT",
    "url": "…",
    "commercial": "yes",          // yes | conditions | no
    "conditions_pl": null,
    "eu_allowed": true,
    "gated": false                // wymaga konta i akceptacji na HF
  },
  "variants": [
    {
      "id": "q5_0",
      "label": "Q5 (zalecany)",
      "files": [
        { "repo": "…", "path": "…", "size": 0, "sha256": "TODO-z-HF-API" }
      ],
      "memory_estimate_mb": 0,
      "platforms": ["windows-x64", "macos-arm64", "android-arm64"],
      "recommended_backends": ["cuda", "metal", "vulkan", "cpu"]
    }
  ],
  "tags": ["polski", "szybki"]
}
```

### 6.2 Wymagania

- Skrypt `tools/catalog-sync` pobiera rozmiary i sumy sha256 z API Hugging Face i uzupełnia manifest. Ręczne wpisywanie sum jest zabronione.
- Modele LLM: możliwość **dodania własnego pliku GGUF** z dysku lub z URL Hugging Face (repo + plik). Szacowanie pamięci z rozmiaru pliku + kontekstu.
- Modele z licencją wykluczającą UE oznaczane wyraźnie; domyślnie ukryte filtrem „Dozwolone w UE”.
- Wstępny zestaw (do weryfikacji aktualności przez Claude Code):
  - LLM: 3–5 modeli GGUF w różnych rozmiarach (?1–4B, ?7–9B, ?12–14B, ?24–32B), w tym co najmniej jeden dobry po polsku.
  - Obraz: 2–3 modele obsługiwane przez stable-diffusion.cpp (np. SDXL, wariant FLUX), z licencją pozwalającą na użytek komercyjny tam, gdzie to możliwe.
  - STT: Whisper (tiny/base/small do słabszych urządzeń, large-v3-turbo jako domyślny).
  - TTS: co najmniej jeden model z **obsługą polskiego i klonowaniem głosu** (np. Chatterbox Multilingual – zweryfikować) oraz jeden lekki TTS bez klonowania (np. Piper, ONNX) działający także na CPU/Androidzie.

---

## 7. Menedżer pobrań

- Kolejka FIFO z możliwością zmiany kolejności, pauzy, wznowienia, anulowania.
- Wznawianie przez HTTP Range; pliki tymczasowe `*.part`.
- Weryfikacja sha256 po pobraniu; plik z błędną sumą nie jest oznaczany jako gotowy (automatyczna ponowna próba raz, potem błąd).
- Obsługa tokenu Hugging Face (przechowywany w systemowym magazynie sekretów: Windows Credential Manager / macOS Keychain) dla modeli gated.
- Przed pobraniem: kontrola wolnego miejsca (rozmiar + 10%).
- Wykrywanie modeli już obecnych (np. w lokalnym buforze HF lub wskazanym katalogu) – bez ponownego pobierania.
- Limit przepustowości (opcjonalny) w ustawieniach.

---

## 8. System zadań i łańcuchy

### 8.1 Zadanie (Job)

Pola: `id`, `type` (chat | image | stt | tts | pipeline), `status` (queued | preparing | loading_model | running | done | failed | cancelled), `progress` (0–1 lub nieokreślony), `eta`, `params`, `inputs`, `outputs`, `error`, `created_at`, `finished_at`.

- Postęp przesyłany do UI zdarzeniami Tauri.
- Anulowanie musi faktycznie przerwać pracę silnika (lub zabić proces, jeśli brak innej drogi).
- Błędy przetłumaczone na komunikaty dla człowieka + „Szczegóły techniczne” z logiem.

### 8.2 Łańcuchy (Pipelines)

- Łańcuch = lista kroków liniowa (bez rozgałęzień w MVP). Każdy krok: tryb + model + parametry + mapowanie wejścia z poprzedniego kroku.
- Typy danych przepływających: `text`, `audio`, `image`, `subtitles`.
- Walidacja zgodności typów między krokami w edytorze.
- Wbudowane szablony:
  1. **Nagranie › napisy › streszczenie** (STT › LLM).
  2. **Nagranie › tłumaczenie › lektor** (STT › LLM tłumaczenie › TTS wybranym głosem).
  3. **Tekst › ilustracja + narracja** (LLM tworzy prompt obrazu i tekst narracji › Obraz + TTS).
- Łańcuch zapisany przez użytkownika można uruchomić z trybu pracy jednym kliknięciem.

---

## 9. Interfejs użytkownika

### 9.1 Zasady

- **Autorski układ** opisany poniżej. Jeden spójny „Warsztat” zamiast osobnych studiów z formularzem po lewej i wynikami po prawej.
- Najpierw prostota: domyślnie widać tylko pole polecenia i wybór trybu. Parametry w wysuwanym panelu.
- Każdy element długotrwały pokazuje stan (kolejka, ładowanie modelu, generowanie) i da się go anulować.
- Kolor tylko dla znaczenia: stan gotowy / w toku / błąd / ostrzeżenie. Reszta stonowana.
- Dostępność: pełna obsługa klawiatury, kontrast WCAG AA, etykiety ARIA, `prefers-reduced-motion`.
- Responsywność: od 360 px (Android) do szerokich ekranów.

### 9.2 Szkielet okna (desktop)

```
-¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¬
- NAGŁÓWEK: logo · wyszukiwarka/paleta poleceń (Ctrl+K) ·       -
-           wskaźnik pamięci [¦¦¦¦?? 9.1/24 GB] · zadania (?2)  -
+¦¦¦¦T¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦T¦¦¦¦¦¦¦¦¦¦¦+
- R  -                                             - INSPEKTOR -
- A  -            OBSZAR GŁÓWNY WIDOKU             - (wysuwany -
- I  -                                             -  z prawej,-
- L  -                                             -  domyślnie-
-    -                                             -  zwinięty)-
+¦¦¦¦+¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦+¦¦¦¦¦¦¦¦¦¦¦+
- (w Warsztacie) PASEK POLECEŃ: [tryb ?] [pole tekstowe/plik] ? -
L¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦¦-
```

- **Rail (wąski pasek ikon po lewej)**: Warsztat, Galeria, Łańcuchy, Głosy, Modele, Silniki, Ustawienia. Ikony + etykiety w tooltipach; możliwość rozwinięcia z etykietami.
- **Nagłówek**: paleta poleceń (Ctrl/Cmd+K – przejście do widoku, uruchomienie łańcucha, zmiana modelu), wskaźnik pamięci (klik › szczegóły załadowanych modeli z przyciskiem „Zwolnij”), ikona zadań z licznikiem (klik › **Tacka aktywności**).
- **Tacka aktywności**: wysuwana od góry-prawej lista zadań i pobrań: postęp, ETA, anuluj, przejdź do wyniku. Widoczna z każdego widoku.
- **Inspektor**: panel parametrów kontekstowych (model, wariant, parametry trybu, seed). Zwinięty domyślnie; zapamiętuje stan.

### 9.3 Widoki

#### V1. Kreator pierwszego uruchomienia
Kroki (pełnoekranowe karty z paskiem postępu kroków):
1. Powitanie + język.
2. Diagnostyka sprzętu (animowane wykrywanie, potem podsumowanie w prostych słowach: „Twój komputer poradzi sobie z: czatem ?, obrazami ?, klonowaniem głosu ? wolniej”).
3. Wybór katalogu danych (z wolnym miejscem na każdym dysku).
4. Pakiet startowy: proponowany zestaw modeli (po jednym na tryb, dopasowanych do sprzętu) z łącznym rozmiarem; checkboksy.
5. Start pobierania › przejście do Warsztatu (pobieranie trwa w tle, widoczne w Tacce).

#### V2. Warsztat (widok główny)
- **Środek**: strumień wyników w formie osi czasu (najnowsze na dole), wspólny dla wszystkich trybów. Każda pozycja to karta zależna od typu:
  - czat: wymiana wiadomości z modelem (markdown, bloki kodu z kopiowaniem, przycisk „zatrzymaj”),
  - obraz: miniatury (siatka przy wielu wariantach), klik › podgląd pełnoekranowy,
  - transkrypcja: tekst z znacznikami czasu, eksport TXT/SRT/VTT/JSON, odtwarzacz zsynchronizowany z tekstem,
  - głos: odtwarzacz z falą, eksport WAV/MP3, „użyj ponownie z innym tekstem”.
- **Pasek poleceń na dole**:
  - przełącznik trybu (Czat / Obraz / Transkrypcja / Głos / Łańcuch) – także skróty Alt+1…5,
  - pole tekstowe (wieloliniowe, Ctrl+Enter = wyślij) lub strefa upuszczania pliku (audio do transkrypcji, próbka głosu, obraz),
  - szybkie chipy zależne od trybu: Obraz – proporcje (1:1, 3:4, 16:9), liczba wariantów; Głos – wybrany profil głosu; Czat – wybrany model,
  - przycisk Wyślij/Generuj; stan „w kolejce” jeśli coś już trwa.
- **Lewa szuflada sesji** (rozwijana z Warsztatu): lista sesji/rozmów, wyszukiwanie, przypinanie, zmiana nazwy. Sesja może zawierać wyniki z różnych trybów.
- **Akcje na kartach**: kopiuj, zapisz jako, wyślij do… (np. tekst odpowiedzi LLM › Głos; transkrypcja › Czat „streść”), powtórz z innym seedem, usuń.
- Pusty stan: krótkie przykłady poleceń dla każdego trybu do kliknięcia.

#### V3. Galeria i historia
- Siatka/lista wszystkich wyników ze wszystkich sesji.
- Filtry: typ, model, data, sesja, łańcuch; wyszukiwanie po prompcie/tekście.
- Panel szczegółów z pełnymi metadanymi i przyciskiem „Odtwórz parametry w Warsztacie”.
- Zaznaczanie wielu, eksport do folderu (ZIP), usuwanie.

#### V4. Modele (katalog)
- Układ: lista kart w jednej kolumnie (szerokie wiersze), u góry pasek filtrów jako „chipy” i wyszukiwarka.
- Filtry: tryb, dopasowanie do sprzętu, licencja komercyjna, dozwolone w UE, język polski, zainstalowane/niezainstalowane.
- Wiersz modelu: nazwa, wydawca, tryb, etykieta dopasowania (sekcja 5), rozmiar, licencja jako odznaka (zielona / bursztynowa „warunki” / szara „niekomercyjnie”), przycisk Pobierz/Usuń/Ustaw jako domyślny.
- Rozwinięcie wiersza (akordeon, nie panel boczny): opis, warianty w tabeli (rozmiar, pamięć, dopasowanie), szczegóły licencji z linkiem, wymagany silnik i jego stan.
- Zakładka „Własne modele”: dodaj GGUF z dysku lub z adresu HF.

#### V5. Silniki
- Lista silników jako karty: nazwa, możliwości, wersja zainstalowana/dostępna, backend (CUDA/Metal/Vulkan/CPU), stan (nie zainstalowany / instalacja / gotowy / uruchomiony / błąd).
- Akcje: Zainstaluj, Aktualizuj, Uruchom ponownie, Odinstaluj, Pokaż logi (podgląd ostatnich linii + otwórz plik).
- Dla silników pythonowych: rozmiar środowiska, przycisk „Napraw środowisko”.

#### V6. Głosy (biblioteka)
- Siatka kart profili: nazwa, język, model docelowy, długość próbki, odsłuch.
- Tworzenie profilu: nagranie z mikrofonu lub upload; przycinanie próbki na fali; wskaźnik jakości (poziom szumu, przesterowanie, długość – zalecane 10–30 s).
- **Obowiązkowe oświadczenie** przy tworzeniu: „To mój głos albo mam zgodę tej osoby” (checkbox, zapisywany z profilem).
- Edycja, duplikacja, eksport/import profilu (archiwum z próbką i metadanymi).

#### V7. Łańcuchy
- Lista łańcuchów (szablony + własne).
- Edytor: pionowa lista kart kroków połączonych strzałką z typem danych („audio › tekst”). Dodaj krok, zmień kolejność przeciąganiem, usuń.
- Każdy krok: tryb, model, parametry, dla LLM – edytowalny szablon promptu z zmienną `{{input}}`.
- Walidacja typów na bieżąco (czerwona strzałka przy niezgodności).
- „Uruchom testowo” z podglądem wyniku każdego kroku.

#### V8. Ustawienia
Sekcje: Ogólne (język, motyw, katalog danych), Sprzęt i pamięć (budżet, współczynnik dla pamięci zunifikowanej, polityka zwalniania), Pobieranie (token HF, limit prędkości), Sieć (udostępnianie API w LAN – domyślnie wyłączone, parowanie urządzeń, lista sparowanych), Prywatność (czyszczenie historii, telemetria: brak), Zaawansowane (logi, porty, tryb debug).

#### V9. Android (faza 1 – tryb klienta)
- Ekran parowania: skan kodu QR wyświetlanego w Ustawieniach › Sieć na desktopie.
- Uproszczony Warsztat: strumień + pasek poleceń, tryby jak na desktopie, wykonanie na sparowanym komputerze.
- Galeria wyników (synchronizowana na żądanie).
- Nagrywanie audio z telefonu do transkrypcji i do próbek głosu.
- Faza 2: przełącznik „Lokalnie na telefonie / Na komputerze” z listą modeli działających na urządzeniu.

### 9.4 Stany i komunikaty
- Każdy widok ma zdefiniowane: stan pusty, ładowanie (szkielety), błąd (z akcją naprawczą), brak modelu („Ten tryb potrzebuje modelu – pobierz zalecany [rozmiar]”).
- Toasty tylko dla zdarzeń zakończonych w tle (pobranie gotowe, zadanie gotowe, błąd).
- Powiadomienia systemowe dla długich zadań, gdy okno jest w tle (opcjonalne).

### 9.5 Tokeny projektowe
- Kolory jako zmienne CSS na `:root`, z wariantem ciemnym (`prefers-color-scheme` + ręczny przełącznik).
- Kolory semantyczne: `--ok`, `--busy`, `--error`, `--warn`, `--info`.
- Skala odstępów 4 px, zaokrąglenia 6/10/16 px, typografia systemowa (bez zewnętrznych CDN – aplikacja działa offline).
- Własna ikonografia (np. Lucide, licencja ISC) – bez zasobów z innych aplikacji.

---

## 10. Wymagania niefunkcjonalne

- **Prywatność**: brak telemetrii. Ruch sieciowy wyłącznie: pobieranie modeli/silników, sprawdzanie aktualizacji (wyłączalne).
- **Bezpieczeństwo**:
  - silniki nasłuchują tylko na 127.0.0.1, na losowym porcie, z tokenem w nagłówku,
  - lokalne API sprawdza `Origin` i token,
  - tryb LAN: osobny port, parowanie, TLS z certyfikatem samopodpisanym przypiętym na kliencie,
  - binaria silników pobierane wyłącznie z oficjalnych wydań, weryfikowane sumą,
  - sanityzacja nazw plików wynikowych.
- **Wydajność**: start aplikacji < 2 s (bez ładowania modeli); UI responsywne podczas generowania; strumieniowanie tokenów LLM bez zacięć.
- **Niezawodność**: crash silnika nie zamyka aplikacji – supervisor restartuje go i oznacza zadanie jako nieudane z komunikatem.
- **Instalator**: Windows – instalacja per użytkownik bez uprawnień administratora (NSIS/MSI z Tauri); macOS – DMG, podpis i notaryzacja (w planie); aktualizacje przez updater Tauri z podpisem.
- **Rozmiar instalatora**: docelowo < 30 MB (silniki i modele dociągane).
- **i18n**: wszystkie teksty w plikach `locales/pl.json`, `locales/en.json`.
- **Logi**: rotowane, w katalogu danych, bez treści promptów w trybie domyślnym.

---

## 11. Aspekty prawne i etyczne

- Licencja każdego modelu widoczna przed pobraniem; modele gated wymagają akceptacji licencji na HF przez użytkownika.
- Filtr „Dozwolone w UE” domyślnie włączony.
- Klonowanie głosu: wymagane oświadczenie o zgodzie; opcjonalny znak w metadanych pliku („wygenerowano lokalnie AI”).
- Wyniki obrazu i audio: metadane z nazwą modelu (wymagane przez niektóre licencje).
- Aplikacja nie dołącza wag modeli do instalatora.

---

## 12. Struktura repozytorium (propozycja)

```
/apps
  /desktop          # Tauri 2: src-tauri (Rust) + frontend
  /mobile           # Tauri 2 Android (faza 1: klient)
/crates
  /core             # registry, downloads, jobs, scheduler, storage
  /engines          # adaptery silników
  /hwprobe          # diagnostyka sprzętu
  /api-gateway      # lokalne API zgodne z OpenAI
/ui                 # Svelte: widoki, komponenty, i18n, tokeny
/workers
  /python-tts       # FastAPI worker, pyproject (uv)
/catalog
  models.json
  engines/*.json
/tools
  /catalog-sync     # pobieranie rozmiarów i sha256 z HF
/docs
  /adr
```

---

## 13. Kamienie milowe i kryteria akceptacji

### M0 – Szkielet
- Tauri 2 + Svelte uruchamia się na Windows i macOS; rail, nagłówek, routing do pustych widoków; i18n PL/EN; motyw jasny/ciemny.
- **Akceptacja**: aplikacja buduje się w CI na obu systemach; przełączanie języka i motywu działa.

### M1 – Sprzęt, katalog, pobieranie
- Diagnostyka (V1 kroki 1–3), katalog (V4) z manifestem, menedżer pobrań z wznawianiem i sha256, Tacka aktywności.
- **Akceptacja**: przerwane pobranie wznawia się po restarcie aplikacji; plik z błędną sumą jest odrzucany; etykiety dopasowania zgodne z wykrytym sprzętem.

### M2 – Silniki + Czat
- Menedżer silników (V5), adapter llama.cpp, tryb Czat w Warsztacie ze strumieniowaniem, sesje, wskaźnik pamięci.
- **Akceptacja**: czat działa na CUDA (Windows) i Metal (macOS); anulowanie przerywa generowanie; awaria silnika nie zamyka aplikacji.

### M3 – Transkrypcja + Obraz
- Adaptery whisper.cpp i stable-diffusion.cpp, karty wyników, Galeria (V3), VRAM scheduler z LRU.
- **Akceptacja**: plik 10 min audio › SRT po polsku; obraz 1024 px generuje się i zapisuje z metadanymi; przełączanie między trybami zwalnia pamięć zgodnie z polityką.

### M4 – Głos
- Python worker (uv), model TTS z polskim i klonowaniem, biblioteka Głosów (V6) z oświadczeniem o zgodzie.
- **Akceptacja**: instalacja silnika pythonowego jednym przyciskiem na czystym systemie; profil głosu z 20 s próbki; synteza polskiego tekstu sklonowanym głosem.

### M5 – Łańcuchy + lokalne API + Android klient
- Edytor łańcuchów (V7) z 3 szablonami, brama API zgodna z OpenAI, tryb LAN z parowaniem, aplikacja Android w trybie klienta (V9).
- **Akceptacja**: szablon „Nagranie › tłumaczenie › lektor” działa end-to-end; telefon po sparowaniu QR wysyła nagranie i odbiera transkrypcję.

### M5b – Modele decyzyjne (ADR 0024)
- Tryb „Decyzje” w Warsztacie (tylko Expert) i krok „Decyzja” w łańcuchach z warunkiem; modele Basal 2.0 przez `/v1/systemone` silnika llama.cpp, w osobnym gnieździe obok modelu czatu.
- **Akceptacja**: na czystym systemie pierwsza decyzja bez wizyty w widoku Silniki; polski tekst i trzy rodzaje pytań (wybór, tak/nie, ocena) z prawdopodobieństwami sumującymi się do 1 na RTX 3090, karcie 8 GB i bez karty graficznej (czasy w ADR 0024); łańcuch „Nagranie › decyzja › …” działa, a zamknięty warunek kończy przebieg i pomija dalsze kroki; modele decyzyjne nie trafiają do wyboru modelu czatu; przy wystarczającej pamięci decyzja nie wyładowuje modelu czatu.

### M5c – Serwer i agenci (ADR 0025, 0026, 0028)
- Panel Serwer (Expert): API zgodne z OpenAI na 127.0.0.1 (po włączeniu 0.0.0.0), klucze dostępu, lista ostatnich zapytań; agenci Goose, OpenCode, Pi, Cline, Hermes Agent, OpenClaw i Droid instalowani i uruchamiani jednym kliknięciem.
- **Akceptacja (serwer)**: klient OpenAI z kluczem dostaje odpowiedź strumieniową i wywołanie narzędzia; bez klucza – 401; pierwsze zapytanie samo instaluje silnik i ładuje model; decyzja przez `/v1/systemone`; czat w Warsztacie i zapytanie do serwera działają naraz; przy wyłączonym udostępnianiu serwer nie odpowiada z sieci. Sprawdzone na RTX 3090, karcie 8 GB i bez karty graficznej.
- **Akceptacja (agenci)**: każdy agent instaluje się na czystym systemie, uruchamia jednym kliknięciem i wykonuje małe zadanie na lokalnym modelu.

### M5d – Muzyka (przed 1.0, ADR 0022)
- Tryb „Muzyka”: piosenki z wokalem z opisu stylu i tekstu (pusty tekst = utwór instrumentalny), silnik `ace-step` (Python, uv) instalowany sam, wynik WAV + MP3 w Galerii.
- **Akceptacja**: na czystym systemie pierwsza piosenka bez wizyty w widoku Silniki; polski tekst zaśpiewany na RTX 3090 i na karcie 8 GB; polski test odsłuchowy zapisany w ADR 0022.

### M6 – Dopracowanie i wydanie 1.0 (ADR 0029, 0030, 0031)
- Instalatory, updater, dostępność, stany puste/błędów, testy E2E (Playwright dla UI, testy integracyjne silników na małych modelach).

### M7 – API i serwer MCP dla agentów (po M6; ADR 0032 i kolejne)
- Trzy PR-y: (1) obrazy, mowa i transkrypcja w lokalnym API zgodnym z OpenAI (ADR 0032); (2) serwer MCP (Streamable HTTP + mały most stdio `lokalomat-mcp`) z narzędziami czat, obraz, transkrypcja, mowa, muzyka, decyzja, łańcuch, lista modeli; pliki jako zasoby MCP (ADR 0033); (3) zgoda użytkownika przy pierwszym połączeniu klienta, lista klientów i instrukcja „Dodaj do Claude Code” jednym kliknięciem (ADR 0034).
- **Akceptacja**: klient OpenAI i Claude Code z kluczem dostają obraz, transkrypcję (także SRT) i mowę; silnik instaluje się sam przy pierwszym zapytaniu; bez klucza 401; nowy klient wymaga zgody w aplikacji (ADR 0034); most `lokalomat-mcp` jest w instalatorach.

---

## 14. Otwarte pytania

1. Identyfikacja wizualna (logo, kolorystyka) dla nazwy „Lokalomat AI Studio”.
2. Framework UI: Svelte 5 (rekomendacja) vs vanilla TS + Web Components.
3. Czy obsługiwać AMD na Windows w obrazach w MVP (Vulkan w stable-diffusion.cpp – wymaga testów wydajności)?
4. Model biznesowy: darmowa aplikacja, wersja Pro (łańcuchy, LAN), czy open source?
5. Licencja kodu aplikacji (MIT / Apache 2.0 / AGPL).
6. Czy katalog modeli ma być aktualizowany zdalnie (podpisany manifest), czy tylko z wydaniami aplikacji?
7. Który model TTS z polskim i klonowaniem wybrać jako domyślny (porównać jakość, licencję, wymagania) – benchmark przed M4.

---

## 15. Uzupełnienia wymagań (od właściciela produktu, 2026-11-11)

1. **Plug and play (jak LM Studio).** Pobieranie i instalacja silników są maksymalnie zautomatyzowane: gdy tryb potrzebuje silnika, aplikacja sama go pobiera, instaluje i uruchamia, pokazując tylko postęp. Użytkownik nie musi wcześniej odwiedzać widoku Silniki (zostaje on dla zaawansowanych).
2. **Krótkie teksty prostym językiem.** Teksty objaśniające mają jedno krótkie zdanie, zwykłe słowa i bez szczegółów technicznych. Przykład: zamiast „Silniki uruchamiają modele lokalnie. Pobieramy je wyłącznie z oficjalnych wydań i sprawdzamy sumą kontrolną” – „Silniki uruchamiają modele sztucznej inteligencji na Twoim komputerze”. Szczegóły techniczne trafiają do podpowiedzi (tooltipów) i logów.
3. **Logowanie diagnostyczne** w trybach Normalny / Szczegółowy (ADR 0004).
4. **Serwery MCP** lokalne (stdio) i zdalne (HTTP) z interfejsem konfiguracji; wzorzec: `mcp-grafana.exe` (etap M2b).
5. **Linux jako trzecia platforma desktop**: Ubuntu 24.04 LTS i 26.04 LTS (x64), obok Windows i macOS – budowane i testowane w CI (ADR 0011).
6. **Tryb Basic / Expert** – dwustanowy przełącznik w prawym górnym rogu. Basic (domyślny) ukrywa część UI zbyt trudną dla przeciętnego użytkownika; Expert pokazuje wszystko (ADR 0013).
7. **Serwery MCP instalują się same** (np. mcp-grafana) – użytkownik podaje tylko adres i token.
8. **Okno „O aplikacji”**: autor Łukasz Poźniak, firma AI Transformation Łukasz Poźniak, strona aitransformation.pl, licencja MIT (plik `LICENSE`).
9. **Tryb Serwer** – w trybie Expert panel serwera zgodnego z API OpenAI, domyślnie na 127.0.0.1, po włączeniu ustawienia na 0.0.0.0; do uruchamiania agentów z lokalnymi modelami, jak w Ollama. Na razie pusty panel z opisem; realizacja w etapie M5c (po modelach decyzyjnych).
10. **Modele decyzyjne (w stylu Jev i Basal)** – modele, które zamiast tekstu zwracają typowaną decyzję (wybór / tak-nie / ocena) z prawdopodobieństwami. Basal (Apache-2.0, polski, na bazie Bielika) uruchamiany silnikiem `basal-serve` (Python, endpoint `/v1/systemone`). Jev jest usługą chmurową (TypeSafe AI) – poza zasadą local-first. Realizacja w etapie M5b, po M5 (korzysta z workera Pythona z M4). **Zmiana (ADR 0024):** Basal 2.0 działa przez `/v1/systemone` w silniku llama.cpp (pliki `-decision.gguf`), bez `basal-serve` i bez Pythona.
11. **Agenci (third party)** – aplikacja instaluje i uruchamia zewnętrznych agentów, którzy używają Lokalomat AI Studio jako serwera inferencji (serwer z punktu 9). Plug and play: pobranie, konfiguracja adresu i klucza, start jednym kliknięciem. Realizacja w etapie M5c razem z serwerem.

Lokalne API: w M5 powstaje minimalny `crates/api-gateway` – tylko to, czego potrzebuje klient Android (parowanie w sieci lokalnej, endpointy używane przez aplikację mobilną). W M5c ten sam gateway rozbudowujemy o panel Serwer (127.0.0.1 / 0.0.0.0, klucze dostępu, pełny zestaw endpointów zgodnych z OpenAI) i agentów.

12. **Parowanie Android ↔ Desktop (M5)** – telefon łączy się z API na komputerze w sieci lokalnej:
    - na komputerze: „Połącz telefon” pokazuje kod QR z adresem IP, portem, odciskiem certyfikatu TLS serwera i jednorazowym tokenem parowania (ważnym ok. 2 min);
    - telefon skanuje kod, łączy się po TLS z przypiętym certyfikatem (z QR) i wysyła swój klucz publiczny;
    - oba ekrany pokazują **ten sam krótki kod porównawczy**, np. `ABC-DEF`, wyliczony z kluczy obu stron i tokenu – użytkownik potwierdza zgodność na komputerze (jak przy parowaniu Bluetooth); niezgodny kod = przerwanie, bo ktoś mógł się wpiąć pomiędzy;
    - po potwierdzeniu komputer wydaje telefonowi osobny klucz dostępu (w magazynie sekretów obu stron); lista sparowanych urządzeń z przyciskiem „Odłącz”;
    - serwer w sieci lokalnej działa tylko po włączeniu w ustawieniach; bez parowania żadne żądanie nie przechodzi.

13. **Biblioteka modeli Ollamy (po M6)** – w trybie Expert ustawienie „Używaj modeli z Ollamy” z folderem biblioteki (domyślnie standardowy folder Ollamy, np. `~/.ollama/models`; do testów `D:\AI\models`):
    - modele Ollamy (manifesty + bloby GGUF) pojawiają się na liście modeli razem z modelami Lokalomat AI Studio, oznaczone ikonką lamy;
    - są tylko do odczytu: można ich używać w czacie, ale nie usuwać ani nie zmieniać – Lokalomat AI Studio nigdy nie zapisuje w folderze Ollamy;
    - pobieranie modeli zawsze trafia do biblioteki Lokalomat AI Studio, nie do biblioteki Ollamy.

14. **Komputery bez dedykowanej karty graficznej** (np. mini PC z grafiką zintegrowaną, jak Blackview MP100): polecamy tylko modele językowe do ok. 4 mld parametrów (Bielik 4.5B zostaje) albo modele MoE z 2–4 mld aktywnych parametrów (np. Qwen3.6 35B-A3B) – szybkość zależy od aktywnych parametrów, a zmieścić się musi cały model w RAM. Modeli dyfuzyjnych do obrazów (np. Z-Image Turbo) na takich komputerach nie polecamy – są za ciężkie. Grafika zintegrowana nie ma własnej pamięci: budżet liczymy z RAM, nie z pamięci zgłaszanej przez system. Dopasowanie i sortowanie modeli (M1) trzeba o to uzupełnić.

15. **Lokalomat AI Studio jako narzędzie dla agentów (po M6)** – lokalne API i serwer MCP udostępniające wszystkie tryby aplikacji: generowanie obrazów, transkrypcję, głos (TTS), czat i łańcuchy. Agent, np. Claude w Claude Code, może poprosić Lokalomat AI Studio o obraz, napisy do nagrania albo lektora, a aplikacja sama dobiera model, pobiera silnik i zwraca plik. Cel: „kombajn” do modeli lokalnych, którego nie mają LM Studio ani Ollama.
    - Serwer MCP (stdio i Streamable HTTP) z narzędziami typu `generate_image`, `transcribe`, `speak`, `chat`, `run_pipeline`, `list_models`; wyniki jako pliki/zasoby (obraz, SRT, WAV) z metadanymi.
    - To samo jako REST (rozszerzenie gatewaya z M5c, zgodne z OpenAI tam, gdzie to możliwe: `/v1/images/generations`, `/v1/audio/transcriptions`, `/v1/audio/speech`).
    - Bezpieczeństwo jak w M5c: domyślnie tylko 127.0.0.1, klucz dostępu, zgoda użytkownika przy pierwszym połączeniu klienta, lista podłączonych klientów, kolejka zadań i limity pamięci przez scheduler (M3).
    - Gotowe instrukcje „Dodaj do Claude Code / innego klienta MCP” jednym kliknięciem.

16. **Artefakty w czacie (po M6)** – model może utworzyć w rozmowie artefakt: dokument HTML pokazywany obok czatu (bezpiecznie, w izolowanej ramce bez dostępu do sieci i aplikacji).
    - Pobieranie z wyborem formatu: HTML, Markdown, PDF albo DOCX.
    - Model dostaje narzędzia do artefaktów: utwórz; czytaj (limit + offset, wyszukiwanie jak grep); zmieniaj fragmenty (zamiana tekstu jak w sed) – bez przepisywania całości, żeby oszczędzać okno kontekstu.
    - Wersje artefaktu zapisywane przy każdej zmianie, z możliwością powrotu.

17. **Transkrypcja plików audio i wideo** – obsługiwane pliki: WAV, AAC, MP3, M4A, M4V, MP4, MKV (stan M3: wszystkie poza M4V; dźwięk z wideo wyciągany bez zewnętrznych programów). Wynik ze znacznikami czasu (SRT, VTT, tekst z czasami) albo bez (czysty tekst) – wybór przy eksporcie i kopiowaniu.

18. **Modele wideo (np. WAN)** – nowy tryb: krótkie klipy z opisu lub z obrazu. Silnik i modele do wyboru po sprawdzeniu aktualnych wydań (ADR); tylko na mocnych kartach graficznych, na słabszych sprzęt oznaczony jako niewystarczający.

19. **Zdjęcia w czacie** – użytkownik dołącza obraz (plik, schowek, przeciągnięcie) i pyta o niego model; obsługujemy modele z widzeniem (np. Gemma 4, Qwen3-VL) przez llama.cpp z plikiem `mmproj`. W katalogu cecha `vision`; bez takiego modelu przycisk dołączania obrazu jest ukryty.

20. **Wbudowane narzędzia** – działają bez konfiguracji, obok serwerów MCP: kalkulator, data i godzina, wyszukiwarka internetowa i pobieranie strony. Narzędzia sieciowe są domyślnie wyłączone, z wyraźną informacją, że zapytanie opuszcza komputer (PRD §10); wyniki traktujemy jako dane, nie polecenia.

21. **Rozmowa o dokumentach (RAG)** – dołączanie dokumentów (PDF, DOCX, TXT, MD) do rozmowy lub do trwałej bazy wiedzy; lokalny model embeddingów, indeks w bazie aplikacji, odpowiedzi z odnośnikami do źródła (plik, strona). Realizuje „RAG na lokalnych dokumentach” z Fazy 2 (§3.2).

22. **Muzyka (przed 1.0, od właściciela produktu, 2026-11-16)** – nowy tryb: piosenki z wokalem, także po polsku, jak w Suno, ale lokalnie. Model domyślny ACE-Step 1.5 (MIT); modele z licencją niekomercyjną (YuE2, MusicGen) nie trafiają do katalogu (ADR 0022). Realizacja w etapie M5d.

Kolejność etapów po uzupełnieniach: M2b → M3 → M4 → M5 → M5b (modele decyzyjne) → M5c (serwer + agenci) → M5d (muzyka, 22) → M6 (wydanie 1.0) → API i serwer MCP dla agentów (punkt 15) → biblioteka Ollamy → artefakty w czacie (16) → transkrypcja audio/wideo (17) → modele wideo (18) → zdjęcia w czacie (19) → wbudowane narzędzia (20) → rozmowa o dokumentach (21).
