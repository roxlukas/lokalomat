//! Masking of secrets (tokens, passwords, API keys) before anything is
//! written to logs or exported in a diagnostics bundle.

use std::sync::LazyLock;

use regex::Regex;

pub const MASK: &str = "***";

static RULES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    let key =
        r"(?:[a-z0-9]+[_-])*(?:token|secret|password|passwd|api[_-]?key|apikey|authorization)";
    vec![
        // JSON: "hf_token": "value"
        (
            Regex::new(&format!(r#"(?i)("{key}"\s*:\s*")[^"]*(")"#)).unwrap(),
            "${1}***${2}",
        ),
        // key=value / key: value (env vars, CLI args, tracing fields)
        (
            Regex::new(&format!(
                r#"(?i)\b({key}\s*[=:]\s*(?:(?:bearer|basic|token)\s+)?)[^\s,;&"']+"#
            ))
            .unwrap(),
            "${1}***",
        ),
        // CLI flags such as `--api-key <value>` (engine launch arguments)
        (
            Regex::new(r"(?i)(--(?:api-key|token|password)\s+)\S+").unwrap(),
            "${1}***",
        ),
        // Authorization headers
        (
            Regex::new(r"(?i)\b(bearer\s+)[A-Za-z0-9._~+/=-]{6,}").unwrap(),
            "${1}***",
        ),
        // Well-known token formats: Hugging Face, Grafana service accounts, OpenAI-style keys
        (Regex::new(r"\bhf_[A-Za-z0-9]{8,}").unwrap(), "hf_***"),
        (Regex::new(r"\bglsa_[A-Za-z0-9_]{8,}").unwrap(), "glsa_***"),
        (Regex::new(r"\bsk-[A-Za-z0-9_-]{16,}").unwrap(), "sk-***"),
        // Access keys of the local server (`lk-<id>.<secret>`, ADR 0025)
        (
            Regex::new(r"\blk-[a-f0-9]+\.[a-f0-9]{16,}").unwrap(),
            "lk-***",
        ),
    ]
});

/// Returns `input` with every recognised secret replaced by [`MASK`].
pub fn redact(input: &str) -> String {
    let mut out = input.to_owned();
    for (re, replacement) in RULES.iter() {
        if re.is_match(&out) {
            out = re.replace_all(&out, *replacement).into_owned();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn masks_known_token_formats() {
        let out = redact("using hf_AbCdEfGhIjKlMnOp and glsa_abcDEF123_456xyz");
        assert!(!out.contains("AbCdEfGh"), "{out}");
        assert!(!out.contains("abcDEF123"), "{out}");
        assert!(out.contains("hf_***") && out.contains("glsa_***"));
    }

    #[test]
    fn masks_access_keys_of_the_local_server() {
        let out =
            redact("key lk-0123456789ab.0123456789abcdef0123456789abcdef0123456789abcdef in a log");
        assert_eq!(out, "key lk-*** in a log");
    }

    #[test]
    fn masks_json_and_key_value_pairs() {
        let out = redact(
            r#"{"hf_token": "secret-value", "name": "x"} GRAFANA_SERVICE_ACCOUNT_TOKEN=abc123 password: hunter2"#,
        );
        assert!(!out.contains("secret-value"), "{out}");
        assert!(!out.contains("abc123"), "{out}");
        assert!(!out.contains("hunter2"), "{out}");
        assert!(
            out.contains(r#""name": "x""#),
            "non-secret fields stay: {out}"
        );
    }

    #[test]
    fn masks_bearer_headers() {
        let out = redact("Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload");
        assert!(!out.contains("eyJhbGci"), "{out}");
    }

    #[test]
    fn masks_cli_secrets() {
        let out = redact("llama-server -m x.gguf --port 4242 --api-key 0123abcd -c 8192");
        assert!(!out.contains("0123abcd"), "{out}");
        assert!(out.contains("--port 4242"));
    }

    #[test]
    fn keeps_ordinary_parameters() {
        let text = "max_tokens=512 ctx=8192 model=qwen";
        assert_eq!(redact(text), text);
    }
}
