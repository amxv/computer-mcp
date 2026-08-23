#!/usr/bin/env bash
set -euo pipefail

identity="Zodex Local Development"
[[ "$(uname -s)" == "Darwin" ]] || {
  printf 'Local code-signing setup requires macOS.\n' >&2
  exit 1
}
keychain="$(security default-keychain -d user \
  | sed -e 's/^[[:space:]]*"//' -e 's/"[[:space:]]*$//')"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/zodex-codesigning.XXXXXX")"

cleanup() {
  /bin/rm -rf "${temporary}"
}
trap cleanup EXIT
command -v openssl >/dev/null || {
  printf 'Local code-signing setup requires openssl.\n' >&2
  exit 1
}

if security find-identity -v -p codesigning "${keychain}" 2>/dev/null \
    | grep -Fq "\"${identity}\""; then
  printf 'Code-signing identity is already ready: %s\n' "${identity}"
  exit 0
fi

if ! security find-certificate -c "${identity}" -p "${keychain}" \
    >"${temporary}/certificate.pem" 2>/dev/null; then
  cat >"${temporary}/openssl.cnf" <<EOF
[req]
distinguished_name = subject
prompt = no
x509_extensions = codesign

[subject]
CN = ${identity}

[codesign]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid
EOF
  openssl req -new -newkey rsa:3072 -x509 -sha256 -days 3650 -nodes \
    -config "${temporary}/openssl.cnf" -keyout "${temporary}/private-key.pem" \
    -out "${temporary}/certificate.pem" >/dev/null 2>&1
  import_password="$(openssl rand -hex 24)"
  openssl pkcs12 -export -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES \
    -passout "pass:${import_password}" \
    -inkey "${temporary}/private-key.pem" -in "${temporary}/certificate.pem" \
    -out "${temporary}/identity.p12"
  security import "${temporary}/identity.p12" -k "${keychain}" \
    -P "${import_password}" -T /usr/bin/codesign >/dev/null
fi

printf 'macOS will request one-time approval to trust the local Zodex signer.\n'
security add-trusted-cert -r trustRoot -p codeSign -k "${keychain}" \
  "${temporary}/certificate.pem"

security find-identity -v -p codesigning "${keychain}" 2>/dev/null \
  | grep -Fq "\"${identity}\"" || {
    printf 'The code-signing identity is still unavailable: %s\n' "${identity}" >&2
    exit 1
  }
printf 'Code-signing identity is ready: %s\n' "${identity}"
printf 'Future source installs will use it automatically.\n'
