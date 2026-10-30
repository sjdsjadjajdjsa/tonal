#!/bin/sh
# Run once: creates a local code signing certificate named "Tonal Local Signing"
# and puts it in your login keychain. build-app.sh then signs Tonal.app with it,
# so macOS keeps Full Disk Access, audio recording and Keychain permissions
# across rebuilds. Running it again does nothing.
#
# TONAL_KEYCHAIN picks another keychain (used by CI), and
# TONAL_KEYCHAIN_PASSWORD its password, so nothing asks for one.
set -eu

name="Tonal Local Signing"
keychain=${TONAL_KEYCHAIN:-$HOME/Library/Keychains/login.keychain-db}

if security find-identity -p codesigning "$keychain" 2>/dev/null | grep -q "\"$name\""; then
    echo "$name is already set up."
    exit 0
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

cat > "$tmp/cert.cnf" <<EOF
[req]
distinguished_name = dn
prompt = no
x509_extensions = ext
[dn]
CN = $name
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
subjectKeyIdentifier = hash
EOF

# The system openssl is LibreSSL. OpenSSL 3 needs -legacy, or security import
# rejects the .p12 it writes.
openssl=/usr/bin/openssl
[ -x "$openssl" ] || openssl=openssl
legacy=
if "$openssl" version | grep -q '^OpenSSL 3'; then
    legacy=-legacy
fi

"$openssl" req -x509 -newkey rsa:2048 -nodes -sha256 -days 3650 \
    -config "$tmp/cert.cnf" -keyout "$tmp/key.pem" -out "$tmp/cert.pem" 2>/dev/null
pass=$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')
# shellcheck disable=SC2086
"$openssl" pkcs12 -export $legacy -name "$name" -inkey "$tmp/key.pem" -in "$tmp/cert.pem" \
    -passout "pass:$pass" -out "$tmp/cert.p12"

security import "$tmp/cert.p12" -k "$keychain" -f pkcs12 -P "$pass" -T /usr/bin/codesign >/dev/null

# Lets codesign use the key without a dialog on every build.
if [ -n "${TONAL_KEYCHAIN_PASSWORD:-}" ]; then
    security set-key-partition-list -S apple-tool:,apple:,codesign: -s -l "$name" \
        -k "$TONAL_KEYCHAIN_PASSWORD" "$keychain" >/dev/null
else
    echo "macOS now asks for your login password once, so codesign may use the new key."
    security set-key-partition-list -S apple-tool:,apple:,codesign: -s -l "$name" "$keychain" >/dev/null
fi

echo "Done: build-app.sh now signs Tonal.app as \"$name\"."
