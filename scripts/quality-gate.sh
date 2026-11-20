#!/bin/sh
# Bramka jakości (quality gate) — opis w docs/quality-gate.md.
#
# Użycie:
#   sh scripts/quality-gate.sh commit   # uruchamiane przez hook .githooks/pre-commit
#   sh scripts/quality-gate.sh pr       # przed wystawieniem pull requesta (lokalnie i w GitHub Actions)
#
# Kolejność: od najtańszego do najdroższego. Pierwszy błąd zatrzymuje bramkę.

set -u

MODE="${1:-}"
case "$MODE" in
  commit|pr) ;;
  *)
    echo "Użycie: sh scripts/quality-gate.sh commit|pr" >&2
    exit 2
    ;;
esac

ROOT="$(git rev-parse --show-toplevel)" || exit 2
cd "$ROOT" || exit 2

fail() {
  echo "" >&2
  echo "BRAMKA NIE PRZESZŁA: $1" >&2
  echo "Napraw przyczynę i spróbuj ponownie. Instrukcja: docs/quality-gate.md" >&2
  exit 1
}

step() {
  echo "→ $1…"
}

need() {
  command -v "$1" >/dev/null 2>&1 \
    || fail "nie znaleziono polecenia $1. $2 Jeśli jest zainstalowane, zamknij i otwórz ponownie terminal lub edytor."
}

# --- Krok 1: synchronizacja AGENTS.md, CLAUDE.md, GEMINI.md -------------------
# W trybie commit porównujemy wersje dodane do commita (indeks Gita),
# w trybie pr — pliki zapisane w ostatnim commicie (HEAD).
if [ "$MODE" = commit ]; then SOURCE=""; else SOURCE="HEAD"; fi

PATTERN=""
for FILE in AGENTS.md CLAUDE.md GEMINI.md; do
  HASH="$(git rev-parse --verify --quiet "$SOURCE:$FILE")" \
    || fail "brak pliku $FILE w commicie. Trzy pliki agentów muszą istnieć."
  if [ -z "$PATTERN" ]; then
    PATTERN="$HASH"
  elif [ "$HASH" != "$PATTERN" ]; then
    fail "pliki AGENTS.md, CLAUDE.md i GEMINI.md się różnią. Wprowadź tę samą zmianę we wszystkich trzech."
  fi
done
echo "✓ Pliki agentów (AGENTS.md, CLAUDE.md, GEMINI.md) są identyczne."

# --- Tryb pr: gałąź, czyste drzewo, aktualny main ------------------------------
if [ "$MODE" = pr ]; then
  BRANCH="$(git rev-parse --abbrev-ref HEAD)"
  # W GitHub Actions pull request jest sprawdzany jako scalenie (detached HEAD) — to w porządku.
  if [ -z "${GITHUB_ACTIONS:-}" ]; then
    [ "$BRANCH" != main ] || fail "jesteś na gałęzi main. Pull request wystawiamy z osobnej gałęzi."
  fi
  [ -z "$(git status --porcelain)" ] \
    || fail "są niezacommitowane zmiany. Zrób commit, żeby bramka sprawdziła dokładnie to, co trafi do pull requesta."

  # Testy mają sprawdzać kod razem z najnowszym main — inaczej zielona bramka
  # nie mówi nic o tym, co powstanie po scaleniu.
  if [ -z "${GITHUB_ACTIONS:-}" ] && git remote get-url origin >/dev/null 2>&1; then
    git fetch --quiet origin main \
      || fail "nie udało się pobrać origin/main (brak sieci?). Bez tego nie wiadomo, czy gałąź zawiera najnowszy main."
    git merge-base --is-ancestor origin/main HEAD \
      || fail "gałąź nie zawiera najnowszego main. Wykonaj: git merge origin/main — i uruchom bramkę ponownie."
    echo "✓ Gałąź zawiera najnowszy main."
  fi
fi

need cargo "Zainstaluj Rusta (rustup)."
need npm "Zainstaluj Node.js."
[ -d node_modules ] || fail "brak katalogu node_modules. Wykonaj: npm ci"

if [ "$MODE" = commit ]; then
  # Lint i testy działają na plikach z dysku. Żeby sprawdziły dokładnie to,
  # co trafia do commita, na dysku nie może być zmian spoza commita.
  git diff --quiet \
    || fail "część zmian nie jest dodana do commita (git add). Dodaj je albo odłóż (git stash --keep-index), żeby bramka sprawdziła dokładnie to, co zapisujesz."
  [ -z "$(git ls-files --others --exclude-standard)" ] \
    || fail "są nowe pliki spoza commita (git status). Dodaj je (git add), usuń albo wpisz do .gitignore."

  STAGED="$(git diff --cached --name-only --diff-filter=ACMRD)"
  changed() { printf '%s\n' "$STAGED" | grep -Eq "$1"; }

  # Krok 2: lint — tylko gdy commit dotyka danej części projektu.
  if changed '\.rs$|Cargo\.(toml|lock)$'; then
    step "Formatowanie Rusta (cargo fmt)"
    cargo fmt --all --check \
      || fail "kod Rusta nie jest sformatowany. Popraw poleceniem: cargo fmt --all — potem dodaj zmiany do commita."
  else
    echo "– Formatowanie Rusta pominięte: commit nie zmienia plików Rusta."
  fi
  if changed '^ui/'; then
    step "Sprawdzenie UI (svelte-check)"
    npm run --silent check || fail "svelte-check zgłosił błędy lub ostrzeżenia."
  else
    echo "– svelte-check pominięty: commit nie zmienia UI."
  fi
  if changed '^catalog/'; then
    step "Katalog modeli (rozmiary i sumy z API Hugging Face)"
    cargo run --quiet -p catalog-sync -- --check \
      || fail "katalog ma pliki bez rozmiaru lub sumy sha256. Uruchom: cargo run -p catalog-sync"
  fi

  # Krok 3: szybkie testy — biblioteki (bez powłoki Tauri, która wymaga zbudowanego UI).
  if changed '\.rs$|Cargo\.(toml|lock)$|^catalog/'; then
    step "Testy Rusta (lokalsi-core, lokalsi-hwprobe, lokalsi-engines)"
    cargo test --quiet -p lokalsi-core -p lokalsi-hwprobe -p lokalsi-engines \
      || fail "testy Rusta nie przechodzą."
  else
    echo "– Testy Rusta pominięte: commit nie zmienia kodu Rusta ani katalogu."
  fi
  if changed '^ui/'; then
    step "Testy UI (vitest)"
    npm test --silent || fail "testy UI nie przechodzą."
  else
    echo "– Testy UI pominięte: commit nie zmienia UI."
  fi
else
  # Krok 2: lint całego projektu.
  step "Formatowanie Rusta (cargo fmt)"
  cargo fmt --all --check \
    || fail "kod Rusta nie jest sformatowany. Popraw poleceniem: cargo fmt --all — potem zrób commit."
  step "Sprawdzenie UI (svelte-check)"
  npm run --silent check || fail "svelte-check zgłosił błędy lub ostrzeżenia."
  step "Katalog modeli"
  cargo run --quiet -p catalog-sync -- --check \
    || fail "katalog ma pliki bez rozmiaru lub sumy sha256. Uruchom: cargo run -p catalog-sync"
  step "Budowanie UI (potrzebne do kompilacji powłoki Tauri)"
  npm run --silent build:ui || fail "nie udało się zbudować UI."
  step "Clippy (ostrzeżenia = błędy)"
  cargo clippy --workspace --all-targets --quiet -- -D warnings \
    || fail "clippy zgłosił ostrzeżenia."

  # Kroki 3–4: wszystkie testy — jednostkowe, integracyjne (pobieranie, logi) i UI.
  step "Wszystkie testy Rusta"
  cargo test --workspace --quiet || fail "testy Rusta nie przechodzą."
  step "Testy UI (vitest)"
  npm test --silent || fail "testy UI nie przechodzą."

  # Krok 5: budowa aplikacji w trybie release (bez instalatora) — wyłapuje błędy
  # linkowania, profilu release i konfiguracji Tauri, których Clippy nie widzi.
  step "Budowa aplikacji (tauri build, bez instalatora)"
  npm run --silent tauri -- build --no-bundle --ci     || fail "nie udało się zbudować aplikacji."
fi

echo "✓ Bramka ($MODE) przeszła."
exit 0
