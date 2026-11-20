#!/bin/sh
# Włącza hook bramki jakości (.githooks/) w tej kopii repozytorium. Opis: docs/quality-gate.md
# Nie nadpisuje innego, wcześniej ustawionego katalogu hooków — wtedy tylko ostrzega.

cd "$(git rev-parse --show-toplevel)" || exit 0

CURRENT="$(git config --get core.hooksPath)"
case "$CURRENT" in
  .githooks)
    echo "✓ Hook bramki jakości jest włączony."
    ;;
  "")
    git config core.hooksPath .githooks
    echo "✓ Włączono hook bramki jakości (core.hooksPath = .githooks)."
    ;;
  *)
    echo "UWAGA: core.hooksPath wskazuje na \"$CURRENT\", nie na .githooks — hook bramki jakości NIE działa."
    echo "Nie nadpisuję tego ustawienia. Zobacz docs/quality-gate.md, sekcja „Włączenie hooka po sklonowaniu”."
    ;;
esac
exit 0
