#!/usr/bin/env python3
"""Pins the shape of CI changes whose failure is silent (everything stays green).

* the site film refreshes after a stable release only, never blocks one, and holds its one write token in `publish` (`SiteVideoRelease`);
* `cache-cleanup.yml` runs on `pull_request_target`, which is safe only while it never checks out
  or runs pull request code and holds nothing but `actions: write`;
* the nightly's and the release candidate's Sentry debug-file upload is skipped on a pull
  request's dry run and nowhere else (a release, the schedule and a dispatched dry run keep it);
* the tests against the bundled FFmpeg and libass run in a job of their own, beside the replays,
  and still run;
* the host FFmpeg and libass builds in the simulator jobs are CACHED where the build scripts really
  write, under a key that names every input the scripts key on (a cache that "hits" and still
  rebuilds, or one that outlives a changed input, both stay green);
* ci.yml starts on every pull request and its `changes` job gates the heavy jobs; both required
  status names report always (`RequiredChecksReportOnEveryPr`);
* ci.yml's `paths` filter (`CiPathFilter`, evaluated by ci/ci_paths.py, the code the `changes` job runs): evaluated with GitHub's rules, a docs-only or
  repository-metadata-only change (issue forms, a PR template, prose, images) starts no run, while
  every file a gate reads (PRIVACY.md, the credits pair, an `include_str!` target, the workflows)
  still does, and no other workflow carries a catch-all `'**'` filter;
* the nightly Homebrew Channel repository: the nightly build generates its manifest (and debug never
  does), a real nightly can only be cut from main, the manifest reaches the release, and the site
  stages the repository and the guide.

Text-level on purpose, like ci/test_ci_timeouts.py (no YAML library on a stock runner).
"""
from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WORKFLOWS = ROOT / ".github/workflows"
sys.path.insert(0, str(ROOT / "ci"))
import ci_paths  # noqa: E402
import deps  # noqa: E402


def text(name):
    return (WORKFLOWS / name).read_text()


def code(name):
    """The workflow without comment lines, so a comment cannot satisfy or trip an assertion."""
    return "\n".join(l for l in text(name).splitlines() if not l.lstrip().startswith("#"))


def job_body(name, job):
    lines = code(name).splitlines()
    start = next(i for i, l in enumerate(lines) if l == f"  {job}:")
    end = next((i for i in range(start + 1, len(lines)) if re.match(r"^  [A-Za-z0-9_-]+:\s*$", lines[i])), len(lines))
    return "\n".join(lines[start:end])




class SentryUpload(unittest.TestCase):


    def test_the_input_defaults_to_uploading_and_the_step_honours_it(self):
        build = code("build-package.yml")
        self.assertRegex(build, r"upload-debug-files:\n(?:        .*\n)*?        type: boolean\n        default: true\n")
        step = build[build.index("Upload debug files to Sentry"):build.index("ELF + packaging assertions")]
        self.assertIn("UPLOAD: ${{ inputs.upload-debug-files }}", step)
        self.assertIn("ci/check-debug-files.py", step)
        self.assertLess(step.index("ci/check-debug-files.py"), step.index('"$UPLOAD" != true'))
        self.assertLess(step.index('"$UPLOAD" != true'), step.index("debug-files upload"))


class BundledLibrarySymbols(unittest.TestCase):
    """A frame inside a bundled `.so` symbolicates only if its unstripped symbols were kept, carry the
    shipped copy's build id, were checked to show symbols, and were uploaded. Every link in that chain
    fails silently (the upload still succeeds), so each is pinned here."""

    def step(self):
        build = code("build-package.yml")
        return build[build.index("Upload debug files to Sentry"):build.index("ELF + packaging assertions")]

    def test_every_symbol_file_is_checked_for_features_before_the_token_guarded_upload(self):
        step = self.step()
        self.assertIn("python3 ci/check-debug-files.py pkg/plxnative.debug pkg/symbols/*.debug", step)
        self.assertLess(step.index("ci/check-debug-files.py"), step.index('"$UPLOAD" != true'))
        self.assertLess(step.index('"$UPLOAD" != true'), step.index("debug-files upload pkg/symbols"))

    def test_libraries_upload_without_sources_and_sentry_crash_is_not_uploaded(self):
        step = self.step()
        self.assertNotIn("sentry-crash", step)
        self.assertNotRegex(step, r"--include-sources[^\n]*pkg/symbols")
        self.assertEqual(len(re.findall(r"debug-files upload", step)), 2)

    def test_the_build_makes_them_and_the_build_asserts_one_per_shipped_library(self):
        build = code("build-package.yml")
        self.assertIn("make ipk symbols symbols-libs", build)
        self.assertIn('pkg/symbols/$(basename "$lib").debug', build)

    def test_the_symbol_files_are_kept_as_a_separate_build_artifact(self):
        build = code("build-package.yml")
        self.assertIn("name: ${{ inputs.artifact }}-symbols", build)
        self.assertIn("path: pkg/symbols/*.debug", build)
        # ...never inside the artifact that becomes the release's dist/.
        main = build[build.index("name: ${{ inputs.artifact }}\n"):build.index("name: ${{ inputs.artifact }}-symbols")]
        self.assertNotIn("pkg/symbols", main)

    def test_the_cross_builds_give_the_libraries_a_build_id_and_ffmpeg_keeps_the_unstripped_copy(self):
        ffmpeg = (ROOT / "ci/build-ffmpeg.sh").read_text()
        # cross branch only: the HOST build links with Apple's ld, which has no --build-id
        self.assertEqual(ffmpeg.count("--build-id=sha1"), 2)   # the comment names it once more
        self.assertIn("--sysroot=\"$SYSROOT\" \\\n    --extra-ldflags='-Wl,--build-id=sha1'", ffmpeg)
        # the copy is taken BEFORE the strip, from the link output, into a directory no lib glob reaches
        keep = ffmpeg.index('cp "$elf" "$PREFIX/lib/unstripped/')
        self.assertLess(keep, ffmpeg.index('"${CROSS}strip" --strip-unneeded "$target"'))
        libass = (ROOT / "ci/build-libass.py").read_text()
        self.assertIn("'-Wl,--build-id=sha1'", libass)
        self.assertIn("if not host:", libass[libass.index("--version-script"):][:400])

    def test_makefile_target_covers_ffmpeg_and_libass_from_their_unstripped_copies(self):
        make = (ROOT / "Makefile").read_text()
        self.assertIn("FFMPEG_UNSTRIPPED_DIR = $(FFMPEG_PREFIX)/lib/unstripped", make)
        self.assertIn("LIBASS_UNSTRIPPED     = vendor/libass-prefix/lib/libass-plx.so.0", make)
        self.assertIn("symbols-libs: $(SO_SYMBOLS)", make)
        self.assertIn("sentry-symbols: symbols symbols-libs", make)
        self.assertRegex(make, r"\.PHONY:[^\n]* symbols-libs ")
        self.assertIn("pkg/symbols/", (ROOT / ".gitignore").read_text())


















class ToolchainPin(unittest.TestCase):
    """`rust-modules/rust-toolchain.toml` is the ONE spelling of the Rust release, so a Dependabot bump of it
    (the `rust-toolchain` entry of dependabot.yml) is a one-line change that cannot leave a stale copy."""

    # The files that must carry the release: the pin itself, and the snapshot / generated table derived from it
    # (ci/gen-third-party-notices.py checks them against the pin, and --derive-toolchain rewrites them).
    DERIVED = {"rust-modules/rust-toolchain.toml", "ci/third-party-crates.json", "THIRD-PARTY-NOTICES.md"}

    def test_dependabot_bumps_the_toolchain_file(self):
        body = (ROOT / ".github/dependabot.yml").read_text()
        entry = body[body.index("- package-ecosystem: rust-toolchain"):]
        entry = entry[:entry.index("\n\n")]
        self.assertIn("directory: /rust-modules", entry)
        self.assertIn("interval: weekly", entry)
        self.assertIn("prefix: Deps", entry)
        self.assertRegex((ROOT / "rust-modules/rust-toolchain.toml").read_text(), r'(?m)^channel = "1\.\d+\.\d+"$')

    def test_no_second_file_spells_the_release(self):
        import rust_toolchain
        pin = rust_toolchain.pinned_channel()
        literal = re.compile(r"(?<![\w.])" + re.escape(pin) + r"(?![\w.]|-)")
        files = subprocess.run(["git", "-C", str(ROOT), "ls-files", "-z"], capture_output=True, text=True, check=True).stdout.split("\0")
        stray = []
        for rel in filter(None, files):
            if rel in self.DERIVED or rel.endswith((".lock", ".svg", ".png", ".jpg", ".ico", ".woff2", ".ttf", ".gz", ".zip")):
                continue
            try:
                body = (ROOT / rel).read_text()
            except (UnicodeDecodeError, OSError):
                continue
            if literal.search(body):
                stray.append(rel)
        self.assertEqual(stray, [], f"{pin} is spelled outside rust-modules/rust-toolchain.toml: read the file instead of copying the version")




class CiPathFilter(unittest.TestCase):
    """ci.yml's `paths` list (the `&ci_paths` anchor, shared by `push` and `pull_request`), run through
    GitHub's matching rules, because a wrong line here fails silently in BOTH directions: a gate-read
    file excluded means a run that should have caught a break never starts, and a prose file left
    in means ~20 runner-minutes per documentation edit (#261, #513).

    The rules (docs.github.com, "Workflow syntax", filter pattern cheat sheet): patterns are
    evaluated in order; a `!` pattern removes what an earlier pattern matched, a later positive
    pattern puts it back; `*` matches any characters but `/`; `**` matches any characters including
    `/`, and `**/` also matches no directory at all; `?` matches one character but `/`."""

    @staticmethod
    def anchor():
        return ci_paths.patterns(text("ci.yml"))

    @staticmethod
    def triggers(path, patterns=None):
        return ci_paths.triggers(path, patterns if patterns is not None else CiPathFilter.anchor())

    def test_only_push_filters_by_path_and_pull_request_starts_on_every_pr(self):
        # A PR the filter skips starts no run, so the required checks never report. The `changes` job
        # applies the same list to the PR's files instead (RequiredChecksReportOnEveryPr).
        self.assertRegex(code("ci.yml"), r"(?m)^  pull_request:\n  workflow_dispatch:")
        self.assertEqual(len(re.findall(r"(?m)^\s+paths:", code("ci.yml"))), 1)
        self.assertEqual(len(re.findall(r"(?m)^\s+paths: &ci_paths$", text("ci.yml"))), 1)
        self.assertNotIn("*ci_paths", code("ci.yml"))
        self.assertIn("paths: &ci_paths", code("ci.yml").split("  pull_request:")[0])

    def test_the_changes_jobs_evaluator_is_this_evaluator(self):
        # ci/ci_paths.py is what the job runs, on the list it reads from ci.yml: same code, same cases.
        def run(files):
            return subprocess.run(
                [sys.executable, str(ROOT / "ci/ci_paths.py")], input="\n".join(files), text=True,
                capture_output=True, check=True).stdout.strip()
        self.assertEqual(run(list(self.MUST_NOT_TRIGGER)), "false")
        self.assertEqual(run(["README.md", "docs/x.md", "Makefile"]), "true")
        for path in self.MUST_TRIGGER:
            self.assertEqual(run([path]), "true", path)
        for path in self.MUST_NOT_TRIGGER:
            self.assertEqual(run([path]), "false", path)
        self.assertEqual(run(["tools/requirements-docs.txt"]), "false")
        self.assertEqual(run([]), "true", "no files learned: fail closed")

    def test_the_matcher_follows_githubs_rules(self):
        t = self.triggers
        self.assertTrue(t("a/b/c.rs", ["**"]))
        self.assertTrue(t("x", ["**"]))
        self.assertFalse(t("a/b.md", ["**", "!a/**"]))
        self.assertTrue(t("a/b.md", ["**", "!a/**", "a/b.md"]), "a later positive pattern re-includes")
        self.assertFalse(t("a/b.md", ["**", "a/b.md", "!a/**"]), "a later negative pattern excludes again")
        self.assertTrue(t("a/b/c.md", ["**", "!a/*.md"]), "`*` does not cross `/`")
        self.assertFalse(t("a/b/c.md", ["**", "!a/**/*.md"]))
        self.assertFalse(t("a/c.md", ["**", "!a/**/*.md"]), "`**/` also matches no directory")
        self.assertFalse(t("README.md", ["**", "!README.md"]))
        self.assertTrue(t("docs/README.md", ["**", "!README.md"]), "a bare name is anchored at the root")

    # Every path here is read by a gate or ships, so skipping the run for it would hide a break.
    MUST_TRIGGER = (
        "rust-modules/ui/src/widgets.rs", "rust-modules/Cargo.lock", "rust-modules/build.rs", "src/main.c",
        "Makefile", ".gitignore", "ci/check-package.py", "ci/check-python-steps.txt",
        "tests/test_harness.py", "tests/README.md", "tools/check-parallel.py", "locales/en/settings.json",
        "pkg/appinfo.json", "assets/icons/play.svg",
        ".github/workflows/ci.yml", ".github/workflows/build-package.yml",
        ".github/actions/host-rust/action.yml",
        ".claude/hooks/outbound-guard.py", ".claude/hooks/outbound-guard-test.py",
        ".agents/skills/wake-tv/wake-tv.sh",
        "PRIVACY.md", "LICENSE", "LICENSING.md", "TRADEMARKS.md", "THIRD-PARTY-NOTICES.md",
        "docs/screenshots/CREDITS.md", "site/credits.html", "site/index.html", "site/ci/index.html",
        "docs/measurements/p1b-logs/pipe_abr_pin_320.log",
    )
    # Prose, images and repository metadata: no gate reads them (the evidence is in ci.yml).
    MUST_NOT_TRIGGER = (
        ".github/ISSUE_TEMPLATE/bug_report.yml", ".github/ISSUE_TEMPLATE/config.yml",
        ".github/FUNDING.yml", ".github/PULL_REQUEST_TEMPLATE.md", ".github/pull_request_template.md",
        ".github/CODEOWNERS", ".github/dependabot.yml",
        "README.md", "CONTRIBUTING.md", "SECURITY.md", "AGENTS.md", "CLAUDE.md",
        "docs/agent-reference.md", "docs/troubleshooting.md", "docs/release-notes/v0.8.0.md",
        "docs/plex-openapi.json", "docs/screenshots/home.jpg", "docs/assets/sentry-wordmark-dark.svg",
        "docs/webosbrew-package.yml", "docs/measurements/m2-verbose-rerun.txt",
        "site/404.html", "site/media/closeup-glass.jpg", "tools/render-doc-page.py",
        "rust-modules/media/src/player/CLAUDE.md", ".agents/skills/ui-sim/SKILL.md",
        ".claude/settings.json", ".claude/workflows/swarm-gate.js", ".claude/agents/doc-claim-auditor.md",
        ".codex/agents/doc-claim-auditor.toml",
    )

    def test_what_a_gate_reads_triggers(self):
        patterns = self.anchor()
        for path in self.MUST_TRIGGER:
            with self.subTest(path):
                self.assertTrue(self.triggers(path, patterns), f"{path} is read by a gate but starts no run")
                self.assertTrue((ROOT / path).exists(), f"{path} is gone: update MUST_TRIGGER")

    def test_prose_images_and_repository_metadata_do_not(self):
        patterns = self.anchor()
        for path in self.MUST_NOT_TRIGGER:
            with self.subTest(path):
                self.assertFalse(self.triggers(path, patterns), f"{path} starts the whole CI for nothing")

    def test_every_compile_time_include_triggers(self):
        # `include_str!`/`include_bytes!` pull a file into the build, so a change to it changes what the
        # crate tests (PRIVACY.md, LICENSE, the locale and fixture files...) compile against.
        patterns, seen = self.anchor(), 0
        include = re.compile(r'include_(?:str|bytes)!\(\s*"([^"]+)"')
        for folder, dirs, files in os.walk(ROOT / "rust-modules"):
            dirs[:] = [d for d in dirs if not d.startswith("target")]
            for name in files:
                if not name.endswith(".rs"):
                    continue
                source = Path(folder, name)
                for target in include.findall(source.read_text(errors="replace")):
                    resolved = (source.parent / target).resolve()
                    try:
                        relative = resolved.relative_to(ROOT).as_posix()
                    except ValueError:
                        continue
                    seen += 1
                    with self.subTest(f"{source.relative_to(ROOT).as_posix()} -> {relative}"):
                        self.assertTrue(self.triggers(relative, patterns), f"{relative} is include_str!ed")
        self.assertGreater(seen, 50)

    def test_only_ci_has_a_catch_all_filter(self):
        # Every other workflow lists the files it builds from, so a documentation push cannot start it.
        for wf in sorted(WORKFLOWS.glob("*.yml")):
            catch_all = re.findall(r"(?m)^\s+- ['\"]?\*\*['\"]?\s*$", code(wf.name))
            self.assertEqual(len(catch_all), 1 if wf.name == "ci.yml" else 0, wf.name)


class RequiredChecksReportOnEveryPr(unittest.TestCase):
    """`main` requires `host checks (NOT a device gate)` and `cross-build (armv7) + artifact assertions`.
    ci.yml starts on every PR; a `changes` job decides whether the heavy jobs run, and each required
    name is an always-running aggregator that is green when they were skipped for a reason."""

    HEAVY = ("host-lint", "host-unit-default", "host-unit-hostsim", "host-python", "host-python-harness", "cross-build", "firetv")
    REQUIRED = {"host-checks": "host checks (NOT a device gate)",
                "cross-build-required": "cross-build (armv7) + artifact assertions"}

    def test_every_heavy_job_waits_for_changes_and_is_gated(self):
        for job in self.HEAVY:
            body = job_body("ci.yml", job)
            with self.subTest(job):
                self.assertRegex(body, r"(?m)^    needs: changes$")
                self.assertRegex(body, r"(?m)^    if: needs\.changes\.outputs\.gated == 'true'$")

    def test_no_job_escapes_the_gate(self):
        jobs = re.findall(r"(?m)^  ([a-z][a-z0-9-]*):$", code("ci.yml").split("\njobs:\n", 1)[1])
        self.assertEqual(sorted(jobs), sorted(("changes", "webos") + self.HEAVY + tuple(self.REQUIRED)))  # webos: the ungated production build

    def test_changes_job_is_cheap_least_privileged_and_fails_closed(self):
        body = job_body("ci.yml", "changes")
        self.assertIn("runs-on: ubuntu-latest", body)
        self.assertIn("pull-requests: read", body)
        self.assertEqual(code("ci.yml").count("pull-requests: read"), 1, "only that job may read pull requests")
        self.assertRegex(body, r"gated: \$\{\{ steps\.gate\.outputs\.gated \}\}")
        self.assertIn("ci/ci_paths.py", body)
        self.assertRegex(body, r"(?m)^          gated=true$")
        self.assertIn("--paginate", body)

    def test_required_names_exist_once_and_run_always(self):
        for job, name in self.REQUIRED.items():
            body = job_body("ci.yml", job)
            with self.subTest(job):
                self.assertIn(f"name: {name}", body)
                self.assertRegex(body, r"(?m)^    if: always\(\)$")
                self.assertIn("needs.changes.outputs.gated", body)
                self.assertIn("needs.changes.result", body)
            self.assertEqual(len(re.findall(rf"(?m)^    name: {re.escape(name)}$", code("ci.yml"))), 1, name)

    def test_the_heavy_cross_build_does_not_carry_the_required_name(self):
        self.assertNotIn("name: cross-build (armv7) + artifact assertions", job_body("ci.yml", "cross-build"))
        self.assertRegex(job_body("ci.yml", "cross-build-required"), r"needs: \[changes, cross-build\]")

    def test_a_skip_is_green_only_when_changes_said_gated_false(self):
        body = job_body("ci.yml", "cross-build-required")
        self.assertIn('[ "$CHANGES" = success ]', body)
        self.assertIn('[ "$BUILD" = success ]', body)
        self.assertIn('[ "$GATED" = false ] && [ "$BUILD" = skipped ]', body)
        host = job_body("ci.yml", "host-checks")
        self.assertIn('[ "$GATED" = false ]', host)
        self.assertIn("=skipped$", host)
        self.assertIn("=success$", host)




if __name__ == "__main__":
    unittest.main()
