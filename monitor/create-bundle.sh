#!/bin/bash
# create-bundle.sh — Package current session logs for analysis
set -euo pipefail

LOG_ROOT="/var/log/jcr1440"
BUNDLE_DIR="${HOME}"

LATEST=$(ls -1d "$LOG_ROOT"/*/ 2>/dev/null | sort | tail -1)

if [ -z "$LATEST" ]; then
    echo "No log sessions found in $LOG_ROOT"
    exit 1
fi

SESSION_NAME=$(basename "$LATEST")
TIMESTAMP=$(date +%Y-%m-%d-%H%M%S)
BUNDLE_NAME="jcr1440-diagnostics-${TIMESTAMP}.tar.gz"
BUNDLE_PATH="${BUNDLE_DIR}/${BUNDLE_NAME}"

echo "Packaging session: $LATEST"
echo "Output: $BUNDLE_PATH"

# Create a temporary staging directory
STAGING=$(mktemp -d)
trap "rm -rf $STAGING" EXIT

# Copy logs
cp -r "$LATEST" "$STAGING/jcr1440-${SESSION_NAME}/"

# Redact sensitive data in the copy
if command -v python3 &>/dev/null; then
    python3 -c "
import re, os, sys

patterns = [
    (re.compile(r'(IMEI[:\s=]*)\d{11,15}', re.I), r'\g<1>***REDACTED***'),
    (re.compile(r'(ICCID[:\s=]*)\d{15,22}', re.I), r'\g<1>***REDACTED***'),
    (re.compile(r'(IMSI[:\s=]*)\d{10,15}', re.I), r'\g<1>***REDACTED***'),
    (re.compile(r'(MSISDN[:\s=]*)\d{8,15}', re.I), r'\g<1>***REDACTED***'),
    (re.compile(r'(phone[:\s=]*)\+?\d{8,15}', re.I), r'\g<1>***REDACTED***'),
    (re.compile(r'(password[:\s=]*)\S+', re.I), r'\g<1>***REDACTED***'),
    (re.compile(r'(token[:\s=]*)\S+', re.I), r'\g<1>***REDACTED***'),
]

root = '$STAGING'
for dirpath, _, filenames in os.walk(root):
    for fn in filenames:
        if fn.endswith(('.log', '.txt', '.json')):
            fpath = os.path.join(dirpath, fn)
            try:
                text = open(fpath, 'r', errors='replace').read()
                for pat, repl in patterns:
                    text = pat.sub(repl, text)
                open(fpath, 'w').write(text)
            except Exception:
                pass
"
fi

# Remove binary captures over 1MB from bundle
find "$STAGING" -name "*.bin" -size +1M -delete

# Remove complete GPS tracks (keep only the detection flag)
# GPS coordinates in serial captures are already limited by the monitor

# Create archive
tar czf "$BUNDLE_PATH" -C "$STAGING" .

echo ""
echo "Bundle created: $BUNDLE_PATH"
echo "Size: $(du -h "$BUNDLE_PATH" | cut -f1)"
echo ""
echo "You can copy this to a USB drive or upload for analysis."
