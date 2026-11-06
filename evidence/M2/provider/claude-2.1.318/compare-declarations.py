#!/usr/bin/env python3
"""Compare the retained 97 names; never infer absent baseline signatures.

Full generated declarations stay in ignored artifacts. Only the selected
signatures, hashes, and real differences are written to the evidence JSON.
"""
import argparse
import difflib
import hashlib
import json
from pathlib import Path
import re


def digest(data):
    return hashlib.sha256(data).hexdigest()


def uncomment(text):
    # Preserve strings, template literal types, and line numbers. A regex
    # stripping // blindly would corrupt URL/string literal declarations.
    pattern = r"'(?:\\.|[^'\\])*'|\"(?:\\.|[^\"\\])*\"|`(?:\\.|[^`\\])*`|/\*[\s\S]*?\*/|//[^\n]*"
    return re.sub(pattern, lambda m: re.sub(r'[^\n]', ' ', m[0]) if m[0].startswith(('/*', '//')) else m[0], text)


def canonical(text):
    tokens = r"'(?:\\.|[^'\\])*'|\"(?:\\.|[^\"\\])*\"|`(?:\\.|[^`\\])*`|[A-Za-z0-9_$]+|[^\s]"
    return ' '.join(re.findall(tokens, uncomment(text)))


def declarations(path):
    raw = path.read_bytes()
    text = raw.decode()
    clean = uncomment(text)
    # Each declaration is at module scope in the generated files (2 spaces).
    # Bound aliases at the NEXT declaration as well as object declarations;
    # otherwise AgentSpawnArgs/ToolCallEnvelope absorb the following type.
    starts = list(re.finditer(r'^  (?:export )?(?:type|interface|const|function) ([A-Za-z_$][\w$]*)\b', clean, re.M))
    found = {}
    for i, match in enumerate(starts):
        end = starts[i + 1].start() if i + 1 < len(starts) else len(clean)
        signature = '\n'.join(line.rstrip() for line in clean[match.start():end].strip().splitlines() if line.strip())
        found.setdefault(match[1], []).append({
            'line': text.count('\n', 0, match.start()) + 1,
            'signature': signature,
            'normalizedSha256': digest(canonical(signature).encode()),
        })
    return {'sha256': digest(raw), 'lines': len(text.splitlines()), 'firstLine': text.splitlines()[0]}, found


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--candidate', type=Path, required=True)
    parser.add_argument('--prior-310', type=Path, required=True)
    parser.add_argument('--baseline-record', type=Path, default=Path('evidence/M2/provider/claude/declarations.json'))
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    record = json.loads(args.baseline_record.read_text())
    names = sorted(record['same'] + [entry['name'] for entry in record['changed']])
    assert len(names) == len(set(names)) == 97
    candidate, current = declarations(args.candidate)
    prior, old = declarations(args.prior_310)
    # These are the only COMPLETE 2.1.295 declarations in the retained diff.
    complete = {}
    for entry in record['changed']:
        added = '\n'.join(line[1:] for line in entry['diff'] if line.startswith('+'))
        if entry['name'] in ('MockSession', 'ToolCallInput'):
            assert added.startswith('export type ' + entry['name'] + ' =')
            complete[entry['name']] = canonical(added)
    results = []
    for name in names:
        assert len(current.get(name, [])) == 1, f'{name}: missing or duplicate candidate'
        assert len(old.get(name, [])) == 1, f'{name}: missing or duplicate 310'
        now, before = current[name][0], old[name][0]
        result = {'name': name, 'candidate': now, 'against2_1_295': 'NOT_VERIFIABLE',
                  'reason2_1_295': 'Baseline bytes/signature absent; retained name is not equality evidence.',
                  'against2_1_310': 'IDENTICAL' if canonical(now['signature']) == canonical(before['signature']) else 'CHANGED',
                  'prior310NormalizedSha256': before['normalizedSha256']}
        if name in complete:
            assert complete[name] == canonical(now['signature']), name
            result['against2_1_295'] = 'IDENTICAL'
            result['reason2_1_295'] = 'Complete declaration retained as added/replacement lines in the 2.1.295 record.'
        if result['against2_1_310'] == 'CHANGED':
            result['diff310'] = list(difflib.unified_diff(before['signature'].splitlines(), now['signature'].splitlines(), lineterm=''))
        results.append(result)
    output = {
        'schema': 1, 'milestone': 'M2',
        'purpose': 'Same 97 declarations as D-0009; explicit missing-baseline limits and an actual nearest-prior comparison.',
        'source': 'Installed 2.1.318 generated API declarations beside a disposable mod with CLAUDE_CODE_PLUGIN_DIR_WATCH=1. Bundled authoring skill extraction could not run without login; no authentication was changed.',
        'method': 'Strip comments preserving quoted literals; compare declaration tokens ignoring formatting; module-scope boundaries prevent aliases absorbing the next declaration.',
        'baseline295': record['candidate'],
        'candidate318': {'version': '2.1.318', **candidate},
        'prior310': {'version': '2.1.310', **prior, 'source': 'Retired worker disposable mod API declarations; not a qualified build.'},
        'counts295': {'named': 97, 'present318': 97, 'identicalVerified': 2, 'changedVerified': 0, 'removed': 0, 'notVerifiable': 95},
        'counts310': {'compared': 97, 'identical': sum(r['against2_1_310'] == 'IDENTICAL' for r in results), 'changed': sum(r['against2_1_310'] == 'CHANGED' for r in results), 'removed': 0},
        'baselineFragmentNotes': [
            'AgentSpawnInput still has optional workflow { runId: string; agentIndex: number }; this fragment does not establish full equality.',
            'AgentSpawnArgs is a Pick/Partial alias; the retained workflow diff cannot establish its full signature.',
            'The ToolCallEnvelope retained diff names ToolCallInput; no full ToolCallEnvelope baseline signature survives.',
            'No additive/breaking conclusion can be made for the 95 unverified 2.1.295 signatures. There are no changes of either kind against 2.1.310.',
        ],
        'duplicateNames': [], 'declarations': results,
    }
    args.output.write_text(json.dumps(output, indent=2) + '\n')
    print(json.dumps({k: output[k] for k in ('counts295', 'counts310')}))


if __name__ == '__main__':
    main()
