#!/bin/sh
set -eu

if [ "${1:-}" = version ]; then
    [ "$#" -eq 1 ]
    printf '%s\n' 'GitVersion: v3.0.6'
    exit 0
fi

expected_identity="https://github.com/thefrcrazy/DmxServerManager/.github/workflows/release.yml@refs/tags/v${DMX_VERSION:?}"
expected_issuer='https://token.actions.githubusercontent.com'

[ "$#" -eq 6 ]
[ "$1" = verify ]
[ "$2" = --certificate-identity ]
[ "$3" = "$expected_identity" ]
[ "$4" = --certificate-oidc-issuer ]
[ "$5" = "$expected_issuer" ]
[ "$6" = "${DMX_IMAGE:?}" ]

if [ "${DMX_COSIGN_TEST_FAIL_AFTER_VERIFY:-0}" = 1 ]; then
    exit 42
fi
