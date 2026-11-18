#!/usr/bin/env python3
"""Audit satu paket Release sebelum upload artifact.

Read-only: archive tidak diekstrak dan binary/helper tidak dijalankan. Hanya
stdlib Python 3, tanpa network dan tanpa dependency proyek. Output sanitasi
berisi status cek, digest, versi, tipe, dan hitungan kategori; nilai match,
isi string binary, uname pribadi, dan path build tidak pernah dicetak.

Audit ini heuristik dan bukan sertifikasi universal atau tanda tangan penerbit.
"""
import argparse
import gzip
import hashlib
import os
import re
import sys
import tarfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ARCHIVE_NAME = "naf-linux-x86_64.tar.gz"
EXPECTED_FILES = {
    "naf-linux-x86_64/naf": ("scripts", None),
    "naf-linux-x86_64/install.sh": ("source", "scripts/install.sh"),
    "naf-linux-x86_64/VERSION": ("version", None),
    "naf-linux-x86_64/config.toml.example": ("source", "config.toml.example"),
    "naf-linux-x86_64/licenses/Oniguruma.txt": ("source", "assets/licenses/Oniguruma.txt"),
    "naf-linux-x86_64/licenses/Sublime-Packages.txt": ("source", "assets/syntaxes/LICENSE"),
}
EXPECTED_DIRS = {"naf-linux-x86_64", "naf-linux-x86_64/licenses"}

CREDENTIAL_PATTERNS = {
    "private-key": rb"-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----",
    "aws-access-key": rb"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b",
    "github-token": rb"\b(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{20,})\b",
    "slack-token": rb"\bxox[abprs]-[A-Za-z0-9-]{10,}\b",
    "slack-webhook": rb"https://hooks\.slack\.com/services/[A-Za-z0-9/]+",
    "google-api-key": rb"\bAIza[0-9A-Za-z_-]{35}\b",
    "stripe-live-key": rb"\b(?:sk|rk)_live_[0-9A-Za-z]{16,}\b",
    "npm-token": rb"\bnpm_[A-Za-z0-9]{36}\b",
    "jwt": rb"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b",
    "pem-base64": rb"\bMII[A-Za-z0-9+/]{40,}={0,2}",
    "url-credentials": rb"[a-z][a-z0-9+.-]{1,10}://[^\s/:@]{1,64}:[^\s/@]{1,64}@",
    "auth-header": rb"(?i)\b(?:authorization\s*[:=]\s*['\"]?(?:bearer|basic|token)\s+[A-Za-z0-9._~+/-]{12,})",
    "secret-assignment": rb"(?i)\b(?:api[_-]?key|secret[_-]?key|client[_-]?secret|auth[_-]?token|access[_-]?token|refresh[_-]?token|private[_-]?key|password|passwd)\b\s*[:=]\s*[\"']?[A-Za-z0-9_\-/+=!@#$%^&*]{12,}",
    "ssh-private-marker": rb"openssh-key-v1|PuTTY-User-Key-File",
}
HOME_PATTERN = re.compile(rb"/(?:home|Users)/([A-Za-z0-9._-]+)(?:/|$)")
WIN_HOME_PATTERN = re.compile(rb"[A-Za-z]:\\\\Users\\\\([A-Za-z0-9._-]+)")
RUNNER_ACCOUNTS = {b"runner"}

_failures = []
_checks = 0


def check(name, ok):
    global _checks
    _checks += 1
    if not ok:
        _failures.append(name)


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def printable(data):
    return b"\n".join(re.findall(rb"[\x20-\x7e]{8,}", data))


def scan_payload(label, data, finding_counts):
    text = printable(data)
    for category, pattern in CREDENTIAL_PATTERNS.items():
        hits = len(re.findall(pattern, text))
        if hits:
            finding_counts[category] = finding_counts.get(category, 0) + hits
    bad_homes = 0
    for match in HOME_PATTERN.finditer(text):
        if match.group(1) not in RUNNER_ACCOUNTS:
            bad_homes += 1
    for match in WIN_HOME_PATTERN.finditer(text):
        if match.group(1) not in RUNNER_ACCOUNTS:
            bad_homes += 1
    if bad_homes:
        finding_counts["builder-path"] = finding_counts.get("builder-path", 0) + bad_homes


def is_elf_x86_64(data):
    return (len(data) >= 20 and data[:4] == b"\x7fELF" and data[4] == 2
            and data[5] == 1 and data[18:20] == b"\x3e\x00")


def main():
    parser = argparse.ArgumentParser(description="Audit paket Release Notefart.")
    parser.add_argument("--archive", required=True)
    parser.add_argument("--checksum", required=True)
    parser.add_argument("--expected-version", required=True)
    args = parser.parse_args()

    archive_bytes = open(args.archive, "rb").read()
    sidecar = open(args.checksum, "rb").read().decode("utf-8", "replace")
    expected = args.expected_version.strip()

    check("archive-name", os.path.basename(args.archive) == ARCHIVE_NAME)
    match = re.fullmatch(r"([0-9a-fA-F]{64})  naf-linux-x86_64\.tar\.gz\n?", sidecar)
    check("sidecar-format", bool(match))
    digest = sha256_bytes(archive_bytes)
    check("archive-sha256", bool(match) and digest == match.group(1).lower())
    try:
        gzip.decompress(archive_bytes)
        gzip_ok = True
    except (OSError, EOFError):
        gzip_ok = False
    check("gzip-integrity", gzip_ok)

    findings = {}
    with tarfile.open(args.archive, "r:gz") as bundle:
        members = bundle.getmembers()
        files = [m for m in members if m.isreg()]
        dirs = [m for m in members if m.isdir()]
        names = [m.name for m in members]
        check("entry-set", {m.name for m in files} == set(EXPECTED_FILES))
        check("directory-set", {m.name for m in dirs} == EXPECTED_DIRS)
        check("entry-count", len(members) == len(EXPECTED_FILES) + len(EXPECTED_DIRS))
        check("no-duplicates", len(names) == len(set(names)))
        check("no-unsupported-types", len(files) + len(dirs) == len(members))
        check("no-unsafe-paths",
              all(not n.startswith("/") and ".." not in n.split("/") for n in names))
        check("metadata-owner-normalized",
              all(m.uid == 0 and m.gid == 0 and not m.uname and not m.gname for m in members))
        check("metadata-no-user-pax",
              all("uname" not in m.pax_headers and "gname" not in m.pax_headers for m in members))

        payload = {}
        for member in files:
            data = bundle.extractfile(member).read()
            payload[member.name] = data
            scan_payload(member.name, data, findings)
        check("payload-nonempty",
              all(payload.get(name) for name in EXPECTED_FILES))
        naf_member = next((m for m in files if m.name == "naf-linux-x86_64/naf"), None)
        check("naf-executable", bool(naf_member) and bool(naf_member.mode & 0o111))
        check("version", payload.get("naf-linux-x86_64/VERSION") == (expected + "\n").encode())
        check("elf-x86_64", is_elf_x86_64(payload.get("naf-linux-x86_64/naf", b"")))
        for name, (kind, source) in EXPECTED_FILES.items():
            if kind != "source":
                continue
            try:
                with open(os.path.join(ROOT, source), "rb") as fh:
                    trusted = fh.read()
            except OSError:
                trusted = None
            check("trusted-content:" + os.path.basename(name), payload.get(name) == trusted)

    check("no-builder-path", findings.get("builder-path", 0) == 0)
    check("no-credential-material",
          not any(cat in findings for cat in CREDENTIAL_PATTERNS))

    if _failures:
        print(f"FAIL package audit: {len(_failures)}/{_checks} checks failed")
        for name in _failures:
            print(f"- {name}")
        return 1

    size = len(archive_bytes)
    print("PASS package audit")
    print(f"archive={ARCHIVE_NAME} sha256={digest} bytes={size}")
    print(f"version={expected} files={len(EXPECTED_FILES)} dirs={len(EXPECTED_DIRS)}")
    print("elf=x86_64-little-endian metadata-only")
    print(f"scan=clean (findings={findings or 'none'})")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception:
        print("FAIL package audit: unexpected internal error")
        sys.exit(1)