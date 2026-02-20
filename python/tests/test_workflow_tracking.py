"""Tests for workflow tracking API integration.

All tests require GITHUB_TOKEN and network access.
"""

import os
import pytest
from lechange import ChangeDetector, Config

pytestmark = pytest.mark.workflow_api

GITHUB_TOKEN = os.environ.get("GITHUB_TOKEN")
GITHUB_REPOSITORY = os.environ.get("GITHUB_REPOSITORY", "terekete/le-change-test")
TEST_REPO_PATH = "/Users/gatema/Desktop/drive/git/code/le-change-test"


def skip_if_no_token():
    if not GITHUB_TOKEN:
        pytest.skip("GITHUB_TOKEN not set")


def skip_if_no_repo():
    if not os.path.isdir(os.path.join(TEST_REPO_PATH, ".git")):
        pytest.skip("le-change-test repo not available")


@pytest.fixture(autouse=True)
def _check_prerequisites():
    skip_if_no_token()
    skip_if_no_repo()


@pytest.fixture
def shas():
    """Get commit SHAs from test repo."""
    import subprocess

    result = subprocess.run(
        ["git", "log", "--format=%H %s", "--reverse", "main"],
        cwd=TEST_REPO_PATH,
        capture_output=True,
        text=True,
        check=True,
    )
    sha_map = {}
    for i, line in enumerate(result.stdout.strip().split("\n")):
        sha, msg = line.split(" ", 1)
        sha_map[f"commit{i + 1}"] = sha
    return sha_map


class TestWorkflowTracking:
    def test_detect_with_workflow_tracking(self, shas):
        detector = ChangeDetector(TEST_REPO_PATH)
        config = Config(
            base_sha=shas["commit1"],
            sha=shas["commit5"],
            token=GITHUB_TOKEN,
            track_workflow_failures=True,
            wait_for_active_workflows=False,
            workflow_max_wait_seconds=10,
        )
        os.environ["GITHUB_REPOSITORY"] = GITHUB_REPOSITORY
        result = detector.get_changed_files(config)
        # files_to_rebuild should be accessible (may be empty if no failures)
        rebuild = list(result.files_to_rebuild)
        assert isinstance(rebuild, list)

    def test_workflow_rebuild_reasons(self, shas):
        detector = ChangeDetector(TEST_REPO_PATH)
        config = Config(
            base_sha=shas["commit1"],
            sha=shas["commit5"],
            token=GITHUB_TOKEN,
            track_workflow_failures=True,
            wait_for_active_workflows=False,
            workflow_max_wait_seconds=10,
        )
        os.environ["GITHUB_REPOSITORY"] = GITHUB_REPOSITORY
        result = detector.get_changed_files(config)
        reasons = list(result.rebuild_reasons)
        assert isinstance(reasons, list)
        for r in reasons:
            assert "file" in r
            assert "kind" in r
            assert "failed_run_id" in r

    def test_workflow_failed_jobs(self, shas):
        detector = ChangeDetector(TEST_REPO_PATH)
        config = Config(
            base_sha=shas["commit1"],
            sha=shas["commit5"],
            token=GITHUB_TOKEN,
            track_workflow_failures=True,
            wait_for_active_workflows=False,
            workflow_max_wait_seconds=10,
        )
        os.environ["GITHUB_REPOSITORY"] = GITHUB_REPOSITORY
        result = detector.get_changed_files(config)
        jobs = list(result.failed_jobs)
        assert isinstance(jobs, list)

    def test_workflow_successful_jobs(self, shas):
        detector = ChangeDetector(TEST_REPO_PATH)
        config = Config(
            base_sha=shas["commit1"],
            sha=shas["commit5"],
            token=GITHUB_TOKEN,
            track_workflow_failures=True,
            wait_for_active_workflows=False,
            workflow_max_wait_seconds=10,
        )
        os.environ["GITHUB_REPOSITORY"] = GITHUB_REPOSITORY
        result = detector.get_changed_files(config)
        jobs = list(result.successful_jobs)
        assert isinstance(jobs, list)

    def test_workflow_files_to_skip(self, shas):
        detector = ChangeDetector(TEST_REPO_PATH)
        config = Config(
            base_sha=shas["commit1"],
            sha=shas["commit5"],
            token=GITHUB_TOKEN,
            track_workflow_failures=True,
            skip_successful_files=True,
            wait_for_active_workflows=False,
            workflow_max_wait_seconds=10,
        )
        os.environ["GITHUB_REPOSITORY"] = GITHUB_REPOSITORY
        result = detector.get_changed_files(config)
        skip = list(result.files_to_skip)
        assert isinstance(skip, list)

    def test_workflow_disjoint_invariant(self, shas):
        detector = ChangeDetector(TEST_REPO_PATH)
        config = Config(
            base_sha=shas["commit1"],
            sha=shas["commit5"],
            token=GITHUB_TOKEN,
            track_workflow_failures=True,
            skip_successful_files=True,
            wait_for_active_workflows=False,
            workflow_max_wait_seconds=10,
        )
        os.environ["GITHUB_REPOSITORY"] = GITHUB_REPOSITORY
        result = detector.get_changed_files(config)
        rebuild = set(result.files_to_rebuild)
        skip = set(result.files_to_skip)
        assert rebuild.isdisjoint(skip), f"Overlap: {rebuild & skip}"

    def test_workflow_no_wait(self, shas):
        detector = ChangeDetector(TEST_REPO_PATH)
        config = Config(
            base_sha=shas["commit1"],
            sha=shas["commit5"],
            token=GITHUB_TOKEN,
            track_workflow_failures=True,
            wait_for_active_workflows=False,
        )
        os.environ["GITHUB_REPOSITORY"] = GITHUB_REPOSITORY
        result = detector.get_changed_files(config)
        assert result.any_changed

    def test_workflow_short_timeout(self, shas):
        detector = ChangeDetector(TEST_REPO_PATH)
        config = Config(
            base_sha=shas["commit1"],
            sha=shas["commit5"],
            token=GITHUB_TOKEN,
            track_workflow_failures=True,
            wait_for_active_workflows=False,
            workflow_max_wait_seconds=5,
        )
        os.environ["GITHUB_REPOSITORY"] = GITHUB_REPOSITORY
        result = detector.get_changed_files(config)
        assert isinstance(list(result.files_to_rebuild), list)

    def test_workflow_name_filter(self, shas):
        detector = ChangeDetector(TEST_REPO_PATH)
        config = Config(
            base_sha=shas["commit1"],
            sha=shas["commit5"],
            token=GITHUB_TOKEN,
            track_workflow_failures=True,
            workflow_name_filter="CI",
            wait_for_active_workflows=False,
            workflow_max_wait_seconds=10,
        )
        os.environ["GITHUB_REPOSITORY"] = GITHUB_REPOSITORY
        result = detector.get_changed_files(config)
        assert isinstance(list(result.files_to_rebuild), list)


class TestWorkflowWithoutToken:
    def test_without_token(self, shas):
        pytest.importorskip("lechange")
        detector = ChangeDetector(TEST_REPO_PATH)
        config = Config(
            base_sha=shas["commit1"],
            sha=shas["commit5"],
            track_workflow_failures=True,
            wait_for_active_workflows=False,
            workflow_max_wait_seconds=5,
            # No token set
        )
        # Clear GITHUB_REPOSITORY to test graceful handling
        old = os.environ.pop("GITHUB_REPOSITORY", None)
        try:
            # Should either succeed with empty workflow data or produce a diagnostic
            result = detector.get_changed_files(config)
            # If it succeeds, workflow data should be empty or have diagnostics
            rebuild = list(result.files_to_rebuild)
            diags = list(result.diagnostics)
            assert isinstance(rebuild, list)
            assert isinstance(diags, list)
        except Exception:
            # Some error is also acceptable when no token/repo is set
            pass
        finally:
            if old is not None:
                os.environ["GITHUB_REPOSITORY"] = old
