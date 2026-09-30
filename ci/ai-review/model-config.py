#!/usr/bin/env python3
"""Read key=value secrets from stdin for OCR config or GitHub access."""
import json
import sys


PROVIDER_KEYS = {
    'openai': 'OPENAI_APIKEY',
    'openai-responses': 'OPENAI_APIKEY',
    'openrouter': 'OPENROUTER_API_KEY',
}


def read_secret(key, lines):
    secret = None
    for number, raw in enumerate(lines, 1):
        line = raw.strip()
        if not line or line.startswith('#'):
            continue
        name, separator, value = line.partition('=')
        if not separator:
            raise ValueError(f'Expected KEY=value on line {number}.')
        if name.strip() != key:
            continue
        if secret is not None:
            raise ValueError(f'{key} is listed more than once.')
        value = value.strip()
        if len(value) >= 2 and value[0] in ('"', "'") and value[-1] == value[0]:
            value = value[1:-1]
        # Values are data: do not run commands or expand shell variables.
        secret = value
    if not secret:
        raise ValueError(f'{key} is missing or empty.')
    return secret


def model_config(provider, model, lines):
    key = PROVIDER_KEYS.get(provider)
    if key is None:
        raise ValueError(
            f'Unsupported REVIEW_PROVIDER: {ascii(provider)}. '
            f'Supported providers: {", ".join(PROVIDER_KEYS)}. '
            'For other providers, use OCR_CONFIG_JSON or OCR_CONFIG_FILE.'
        )
    if not model.strip():
        raise ValueError('Model is required.')
    api_key = read_secret(key, lines)
    # OCR validates --model against its built-in list plus this models list.
    return {'providers': {provider: {'api_key': api_key, 'model': model, 'models': [model]}}}


if __name__ == '__main__':
    try:
        if sys.argv[1:] == ['--github-token']:
            print(read_secret('GITHUB_TOKEN', sys.stdin))
        elif len(sys.argv) == 3:
            print(json.dumps(model_config(sys.argv[1], sys.argv[2], sys.stdin)))
        else:
            print('Usage: model-config.py PROVIDER MODEL < secrets.env', file=sys.stderr)
            print('       model-config.py --github-token < secrets.env', file=sys.stderr)
            raise SystemExit(2)
    except (OSError, ValueError) as error:
        print(f'Cannot read review secrets: {error}', file=sys.stderr)
        raise SystemExit(1)
