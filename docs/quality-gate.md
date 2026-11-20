# Bramka jakości

> Źródło prawdy dla sprawdzeń, przez które przechodzi każda zmiana, zanim trafi do pull requesta.
> Wzorowane na bramce z projektu CRM. Pliki `AGENTS.md`, `CLAUDE.md` i `GEMINI.md` tylko tu odsyłają.

## Zasada

Każda zmiana przechodzi bramkę, zanim trafi do pull requesta. **Gdy bramka nie przechodzi, naprawiamy przyczynę. Bramki nie omijamy** — nie używamy `git commit --no-verify`, nie wyłączamy hooka, nie usuwamy ani nie osłabiamy sprawdzeń, żeby „przepchnąć” zmianę. Zmiana samej bramki to osobny pull request z uzasadnieniem, za zgodą właściciela.

## Kroki — od najtańszego do najdroższego

| # | Sprawdzenie | Co sprawdza | Narzędzie |
|---|-------------|-------------|-----------|
| 1 | Synchronizacja plików agentów | `AGENTS.md`, `CLAUDE.md` i `GEMINI.md` są identyczne | porównanie w Gicie |
| 2 | Lint | formatowanie Rusta, typy i dostępność w UI (ostrzeżenia = błędy), kompletność katalogu modeli, Clippy | `cargo fmt --check`, `svelte-check`, `catalog-sync --check`, `cargo clippy -D warnings` |
| 3 | Testy jednostkowe | logika rdzenia (dopasowanie modeli, katalog, ustawienia, logi, maskowanie sekretów), UI (filtry, sortowanie, i18n) | `cargo test`, `vitest` |
| 4 | Testy integracyjne | wznawianie pobierania po restarcie, odrzucenie złej sumy sha256, przełączanie logów w locie | `cargo test --workspace` |
| 5 | Budowa aplikacji | aplikacja kompiluje się i linkuje w trybie release, konfiguracja Tauri jest poprawna | `npm run tauri -- build --no-bundle --ci` |
| 6 | Review „high” | dokładny przegląd poprawności pull requesta | `/code-review high <numer PR>` |

Pierwszy błąd zatrzymuje bramkę — dalsze, droższe kroki się nie uruchamiają.

## Gdzie co działa

| Moment | Kroki | Jak uruchamiany |
|--------|-------|-----------------|
| Każdy commit | 1, a dalej tylko dla zmienionych części: fmt + testy bibliotek Rusta, svelte-check + vitest, sprawdzenie katalogu | automatycznie, hook `.githooks/pre-commit` → `scripts/quality-gate.sh commit` |
| Przed wystawieniem pull requesta | 1–5 dla całego projektu (z Clippy, budową UI i budową aplikacji) | ręcznie: `sh scripts/quality-gate.sh pr` |
| Po wystawieniu pull requesta | 6 | agent: review high, wyniki dla właściciela |

**GitHub Actions jest wyłączony** (decyzja właściciela z 2026-11-16, brak rozliczeń na koncie GitHub). Workflowy `.github/workflows/quality-gate.yml` i `ci.yml` zostają w repozytorium, ale się nie uruchamiają — jedynym sprawdzeniem jest lokalna bramka. Lokalnie sprawdzany jest tylko system, na którym ją uruchamiamy; macOS i Ubuntu nie są teraz sprawdzane automatycznie (Ubuntu można sprawdzić, uruchamiając `sh scripts/quality-gate.sh pr` na komputerze z Ubuntu). Ponowne włączenie: `gh workflow enable CI` i `gh workflow enable "Bramka jakości"`.

Warunki trybów:

- **`commit`**: na dysku nie może być zmian spoza commita — ani niedodanych zmian, ani nowych, niedodanych plików. Testy działają na plikach z dysku, więc tylko wtedy sprawdzają dokładnie to, co zapisujemy.
- **`pr`**: nie jesteśmy na `main`, wszystkie zmiany są zacommitowane, a gałąź zawiera najnowszy `origin/main` — testy sprawdzają kod w takim stanie, w jakim znajdzie się po scaleniu. W GitHub Actions dwa ostatnie warunki zapewnia sam GitHub (sprawdza scalenie PR z `main`).

Wymagane narzędzia: Rust (`cargo`, z `rustfmt` i `clippy`), Node.js z `npm ci` (katalog `node_modules`). Testy pobierania używają lokalnego serwera HTTP — sieć nie jest potrzebna. Test z prawdziwym Hugging Face (`hf_live`) jest poza bramką (`--ignored`).

### Sprawdzenie po stronie GitHuba

Hook na komputerze można nieświadomie ominąć (hook niewłączony po sklonowaniu, program bez obsługi hooków). Dlatego ta sama bramka (`scripts/quality-gate.sh pr`) może działać w GitHub Actions na każdym pull requeście do `main`, jako sprawdzenie „Bramka jakości” — obecnie workflow jest wyłączony (patrz wyżej).

Żeby czerwonej bramki nie dało się scalić, właściciel włącza raz w GitHubie: **Settings → Branches → reguła dla `main` → Require status checks to pass → „quality-gate”**. To ustawienie konta — agent go nie zmienia sam.

## Włączenie hooka po sklonowaniu

Git ze względów bezpieczeństwa nie uruchamia hooków z repozytorium automatycznie. Włącza się go raz na każdą kopię repozytorium:

```bash
sh scripts/enable-hooks.sh
```

Skrypt ustawia `core.hooksPath` na `.githooks`. Jeśli wcześniej ustawiono tam inny katalog hooków, **nie nadpisuje go**, tylko ostrzega.

- **Claude Code** uruchamia ten skrypt sam na początku każdej sesji (hook `SessionStart` w `.claude/settings.json`).
- **Każdy inny agent i człowiek** wykonuje go przed pierwszym commitem.
- Sprawdzenie: `git config core.hooksPath` musi wypisać `.githooks`.

Na Windowsie hook działa w powłoce `sh` z Git for Windows. Skrypty mają końce linii LF (`.gitattributes`).

## Co robić, gdy bramka nie przechodzi

Bramka kończy się komunikatem `BRAMKA NIE PRZESZŁA: …` z przyczyną. Commit nie powstaje albo pull request nie jest wystawiany.

| Komunikat | Przyczyna | Co zrobić |
|-----------|-----------|-----------|
| pliki AGENTS.md, CLAUDE.md i GEMINI.md się różnią | zmiana trafiła tylko do części plików | wprowadź tę samą zmianę we wszystkich trzech (`git diff --no-index AGENTS.md CLAUDE.md` nic nie może wypisać) i dodaj wszystkie trzy do commita |
| brak pliku … w commicie | plik agentów usunięty lub niedodany | przywróć plik z identyczną treścią |
| kod Rusta nie jest sformatowany | kod niezgodny z `rustfmt` | `cargo fmt --all`, potem `git add` i ponowny commit |
| svelte-check zgłosił błędy lub ostrzeżenia | błąd typów, dostępności albo nieużywany kod w UI | `npm run check`, popraw wskazane miejsca |
| katalog ma pliki bez rozmiaru lub sumy sha256 | edycja `catalog/models.json` bez synchronizacji | `cargo run -p catalog-sync` (sumy i rozmiary tylko z API Hugging Face) |
| nie udało się zbudować aplikacji | błąd linkowania, profilu release albo `tauri.conf.json` | `npm run tauri -- build --no-bundle` i przeczytaj pierwszy błąd |
| clippy zgłosił ostrzeżenia | kod Rusta do poprawy | `cargo clippy --workspace --all-targets -- -D warnings` i popraw wskazane miejsca |
| testy … nie przechodzą | zmiana coś zepsuła albo test jest nieaktualny | przeczytaj, który test pada; napraw kod. Test zmieniaj tylko wtedy, gdy celowo zmieniło się zachowanie — i napisz to w opisie pull requesta |
| nie znaleziono polecenia cargo / npm | brak narzędzia albo terminal go jeszcze nie widzi | zainstaluj Rusta / Node.js; jeśli są — zamknij i otwórz ponownie terminal |
| brak katalogu node_modules | nie zainstalowano zależności UI | `npm ci` |
| jesteś na gałęzi main | praca bezpośrednio na `main` | przenieś zmiany na gałąź `feature/…`, `fix/…` lub `docs/…` |
| są niezacommitowane zmiany | tryb `pr` przed commitem | zrób commit (przejdzie przez hook), potem ponownie `sh scripts/quality-gate.sh pr` |
| część zmian nie jest dodana do commita | przy commicie na dysku są zmiany spoza commita | `git add` albo `git stash --keep-index`, potem commit |
| są nowe pliki spoza commita | nowe pliki bez `git add` | dodaj je, usuń albo wpisz do `.gitignore` |
| gałąź nie zawiera najnowszego main | `main` zmienił się od utworzenia gałęzi | `git merge origin/main`, rozwiąż konflikty, ponownie `sh scripts/quality-gate.sh pr` |
| nie udało się pobrać origin/main | brak sieci lub dostępu do GitHuba | przywróć połączenie i uruchom ponownie |
| czerwone sprawdzenie „Bramka jakości” w pull requeście | to samo co wyżej, tylko w GitHub Actions | otwórz szczegóły sprawdzenia, przeczytaj komunikat `BRAMKA NIE PRZESZŁA`, napraw na tej samej gałęzi i wypchnij nowy commit |

Jeśli agent nie potrafi naprawić przyczyny, **zatrzymuje się i opisuje problem właścicielowi** prostym językiem. Nie omija bramki i nie wystawia pull requesta.
