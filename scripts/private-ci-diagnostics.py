#!/usr/bin/env python3
"""Report only allowlisted metadata from a private Rust command log."""

import hashlib
from pathlib import Path
import re
import sys


def report_compile(output):
    codes = sorted(set(re.findall(r'error\[E[0-9]{4}\]', output)))
    if codes:
        print(f'::error::Rust diagnostic codes: {" ".join(codes)}.')
    categories = [
        ('No space left on device', 'insufficient disk space'),
        (r'signal: 9, SIGKILL|ld terminated with signal 9|Killed signal terminated program',
         'compiler or linker killed by SIGKILL'),
        (r'undefined reference to|undefined symbol:', 'unresolved linker symbols'),
        ('proc macro panicked', 'procedural macro failed'),
    ]
    for pattern, description in categories:
        if re.search(pattern, output):
            print(f'::error::Rust compilation: {description}.')


def report_tests(output):
    names = set(re.findall(r'^test (.+?) \.\.\. FAILED\r?$', output, re.MULTILINE))
    for fingerprint in sorted(hashlib.sha256(name.encode()).hexdigest() for name in names):
        print(f'::error::Rust test failure fingerprint: {fingerprint}')
    panics = re.findall(
        r"^thread '([^'\r\n]+)'(?: \([0-9]+\))? panicked at [^\r\n]+:([0-9]+):([0-9]+):\r?$",
        output, re.MULTILINE)
    locations = {(hashlib.sha256(name.encode()).hexdigest(), int(line), int(column))
                 for name, line, column in panics}
    for fingerprint, line, column in sorted(locations):
        print(f'::error::Rust test panic location: {fingerprint} {line}:{column}')


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in ('compile', 'tests'):
        print('Usage: private-ci-diagnostics.py compile|tests LOGFILE', file=sys.stderr)
        return 2
    try:
        output = Path(sys.argv[2]).read_text(errors='replace')
    except OSError:
        print('Private Rust diagnostic log is unavailable.', file=sys.stderr)
        return 1
    if sys.argv[1] == 'compile':
        report_compile(output)
    else:
        report_tests(output)
    return 0


if __name__ == '__main__':
    sys.exit(main())
