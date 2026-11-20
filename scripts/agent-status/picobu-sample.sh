#!/bin/sh
# A sample Picobu-style integration: report each phase of an agent run.
#   sh scripts/agent-status/picobu-sample.sh
. "$(dirname "$0")/rodtty-status.sh"
rodtty_status working
echo "picobu: thinking…"; sleep "${PICOBU_STEP:-2}"
rodtty_status waiting
printf 'picobu: continue? [y/N] '; read -r answer
rodtty_status working
echo "picobu: applying ($answer)"; sleep "${PICOBU_STEP:-2}"
rodtty_status finished 30
echo "picobu: done"; sleep "${PICOBU_STEP:-2}"
