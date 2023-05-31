#!/usr/bin/env bash

set -e
f=/usr/share/cozo/cozo.db
# if db file does not exist, create one. (cozo should handle this but does not.)
if [ ! -f "$f" ]; then
  sqlite3 "$f" "VACUUM;"
fi
"$1" server -e sqlite -p "$f"
