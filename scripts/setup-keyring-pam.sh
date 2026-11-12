#!/bin/sh
set -e

# Setup PAM for KWallet auto-unlock at login
# Run with: sudo ./scripts/setup-keyring-pam.sh

if [ "$(id -u)" -ne 0 ]; then
    echo "Error: This script must be run as root (use sudo)." >&2
    exit 1
fi

PAM_FILE=""
for candidate in /etc/pam.d/sddm /etc/pam.d/plasmalogin /etc/pam.d/login; do
    if [ -f "$candidate" ]; then
        PAM_FILE="$candidate"
        break
    fi
done

if [ -z "$PAM_FILE" ]; then
    PAM_FILE="/etc/pam.d/login"
fi

echo "Configuring PAM file: $PAM_FILE"

# Backup original
cp -n "$PAM_FILE" "$PAM_FILE.vide-backup"

# Check if already present
if grep -q "pam_kwallet5.so" "$PAM_FILE"; then
    echo "pam_kwallet5 is already configured in $PAM_FILE."
    exit 0
fi

# Determine kwallet module
MODULE="pam_kwallet5.so"
if [ ! -f "/usr/lib/security/$MODULE" ] && [ -f "/usr/lib/security/pam_kwallet.so" ]; then
    MODULE="pam_kwallet.so"
fi

# Append auth and session hooks
echo "-auth     optional     $MODULE" >> "$PAM_FILE"
echo "-session  optional     $MODULE auto_start force" >> "$PAM_FILE"

echo "Successfully added $MODULE to $PAM_FILE."
echo "Backup preserved at $PAM_FILE.vide-backup."
echo "Your keyring will now auto-unlock on login with matching password."
