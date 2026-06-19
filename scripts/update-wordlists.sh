#!/usr/bin/env bash
# Refresh NYT Wordle word lists from community-maintained sources.
#
# answers.txt merges:
#   - fredoverflow/wordle NYT answer pool (~2,309 canonical solution words)
#   - eithan/wordlelist past answers + today/yesterday (NYT API, daily updates)
#
# Usage:
#   ./scripts/update-wordlists.sh          # download, validate, run tests
#   ./scripts/update-wordlists.sh --dry-run  # download to temp files and report diff only
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATA="$ROOT/data"
ANSWERS="$DATA/answers.txt"
GUESSES="$DATA/allowed_guesses.txt"

ANSWERS_BASE_URL="https://raw.githubusercontent.com/fredoverflow/wordle/master/wordle-nyt-answers-alphabetical.txt"
WORDLELIST_BASE="https://raw.githubusercontent.com/eithan/wordlelist/main"
GUESSES_URL="https://gist.githubusercontent.com/kcwhite/bb598f1b3017b5477cb818c9b086a5d9/raw/wordle_possibles.txt"

DRY_RUN=0
SKIP_TESTS=0

for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    --skip-tests) SKIP_TESTS=1 ;;
    -h|--help)
      echo "Usage: $0 [--dry-run] [--skip-tests]"
      echo "  --dry-run     Fetch and validate without overwriting data/"
      echo "  --skip-tests  Skip cargo test after update"
      exit 0
      ;;
    *)
      echo "Unknown option: $arg" >&2
      exit 1
      ;;
  esac
done

count_lines() {
  wc -l < "$1" | tr -d ' '
}

validate_file() {
  local file="$1"
  local label="$2"
  local bad
  bad="$(grep -Ev '^[a-z]{5}$' "$file" | grep -v '^$' || true)"
  if [[ -n "$bad" ]]; then
    echo "error: $label contains invalid lines:" >&2
    echo "$bad" | head -5 >&2
    exit 1
  fi
  local n
  n="$(count_lines "$file")"
  if [[ "$n" -lt 1000 ]]; then
    echo "error: $label has suspiciously few entries ($n)" >&2
    exit 1
  fi
}

download() {
  local url="$1"
  local dest="$2"
  echo "→ $url"
  if ! curl -fsSL "$url" -o "$dest"; then
    echo "error: failed to download $url" >&2
    exit 1
  fi
}

# Emit lowercase five-letter words from a line-based file or a single-word file.
emit_words() {
  local file="$1"
  if [[ ! -s "$file" ]]; then
    return 0
  fi
  if grep -q $'\n' "$file"; then
    tr '[:upper:]' '[:lower:]' < "$file" | grep -E '^[a-z]{5}$'
  else
    local word
    word="$(tr '[:upper:]' '[:lower:]' < "$file")"
    if [[ "$word" =~ ^[a-z]{5}$ ]]; then
      printf '%s\n' "$word"
    fi
  fi
}

merge_answers() {
  {
    emit_words "$TMP/answers_base.txt"
    emit_words "$TMP/wl_words.txt"
    emit_words "$TMP/wl_current.txt"
    emit_words "$TMP/wl_prior.txt"
  } | sort -u
}

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

download "$ANSWERS_BASE_URL" "$TMP/answers_base.txt"
download "$WORDLELIST_BASE/words.txt" "$TMP/wl_words.txt"
download "$WORDLELIST_BASE/current.txt" "$TMP/wl_current.txt"
download "$WORDLELIST_BASE/prior.txt" "$TMP/wl_prior.txt"
download "$GUESSES_URL" "$TMP/allowed_guesses.txt"

merge_answers > "$TMP/answers.txt"

validate_file "$TMP/answers.txt" "answers"
validate_file "$TMP/allowed_guesses.txt" "allowed_guesses"

old_answers=0
old_guesses=0
[[ -f "$ANSWERS" ]] && old_answers="$(count_lines "$ANSWERS")"
[[ -f "$GUESSES" ]] && old_guesses="$(count_lines "$GUESSES")"

new_answers="$(count_lines "$TMP/answers.txt")"
new_guesses="$(count_lines "$TMP/allowed_guesses.txt")"

echo
echo "answers.txt:         $old_answers → $new_answers  ($(( new_answers - old_answers )) change)"
echo "allowed_guesses.txt: $old_guesses → $new_guesses  ($(( new_guesses - old_guesses )) change)"

if [[ -f "$ANSWERS" ]]; then
  added_answers="$(comm -13 <(sort "$ANSWERS") <(sort "$TMP/answers.txt") | wc -l | tr -d ' ')"
  removed_answers="$(comm -23 <(sort "$ANSWERS") <(sort "$TMP/answers.txt") | wc -l | tr -d ' ')"
  echo "  answers added: $added_answers  removed: $removed_answers"
fi

if [[ -f "$GUESSES" ]]; then
  added_guesses="$(comm -13 <(sort "$GUESSES") <(sort "$TMP/allowed_guesses.txt") | wc -l | tr -d ' ')"
  removed_guesses="$(comm -23 <(sort "$GUESSES") <(sort "$TMP/allowed_guesses.txt") | wc -l | tr -d ' ')"
  echo "  guesses added: $added_guesses  removed: $removed_guesses"
fi

if [[ "$DRY_RUN" -eq 1 ]]; then
  echo
  echo "Dry run — data/ not modified."
  exit 0
fi

mkdir -p "$DATA"
cp "$TMP/answers.txt" "$ANSWERS"
cp "$TMP/allowed_guesses.txt" "$GUESSES"
echo
echo "Updated $ANSWERS and $GUESSES"

if [[ "$SKIP_TESTS" -eq 0 ]]; then
  echo
  echo "Running cargo test..."
  (cd "$ROOT" && cargo test)
  echo
  echo "Word lists updated and tests passed."
else
  echo "Skipped tests (--skip-tests)."
fi
