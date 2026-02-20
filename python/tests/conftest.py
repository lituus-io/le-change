"""
Shared fixtures for LeChange Python tests.
"""

import os
import subprocess
import pytest


@pytest.fixture
def tmp_git_repo(tmp_path):
    """Create an empty git repo with an initial commit."""
    repo = tmp_path / "repo"
    repo.mkdir()
    subprocess.run(["git", "init"], cwd=repo, check=True, capture_output=True)
    subprocess.run(
        ["git", "config", "user.name", "Test"], cwd=repo, check=True, capture_output=True
    )
    subprocess.run(
        ["git", "config", "user.email", "test@test.com"], cwd=repo, check=True, capture_output=True
    )
    # Initial commit
    (repo / "init.txt").write_text("init")
    subprocess.run(["git", "add", "."], cwd=repo, check=True, capture_output=True)
    subprocess.run(
        ["git", "commit", "-m", "Initial commit"], cwd=repo, check=True, capture_output=True
    )
    return repo


@pytest.fixture
def tmp_git_repo_with_changes(tmp_git_repo):
    """Repo with added files in a second commit."""
    repo = tmp_git_repo
    src = repo / "src"
    src.mkdir()
    (src / "main.py").write_text("print('hello')")
    (src / "util.py").write_text("def helper(): pass")
    tests = repo / "tests"
    tests.mkdir()
    (tests / "test_main.py").write_text("def test_main(): assert True")
    subprocess.run(["git", "add", "."], cwd=repo, check=True, capture_output=True)
    subprocess.run(
        ["git", "commit", "-m", "Add source files"], cwd=repo, check=True, capture_output=True
    )
    return repo


@pytest.fixture
def tmp_git_repo_with_deletion(tmp_git_repo_with_changes):
    """Repo where a file was deleted in the latest commit."""
    repo = tmp_git_repo_with_changes
    subprocess.run(
        ["git", "rm", "src/util.py"], cwd=repo, check=True, capture_output=True
    )
    subprocess.run(
        ["git", "commit", "-m", "Delete util.py"], cwd=repo, check=True, capture_output=True
    )
    return repo


@pytest.fixture
def tmp_git_repo_with_rename(tmp_git_repo_with_changes):
    """Repo with a renamed file in the latest commit."""
    repo = tmp_git_repo_with_changes
    subprocess.run(
        ["git", "mv", "src/util.py", "src/helpers.py"],
        cwd=repo,
        check=True,
        capture_output=True,
    )
    subprocess.run(
        ["git", "commit", "-m", "Rename util.py to helpers.py"],
        cwd=repo,
        check=True,
        capture_output=True,
    )
    return repo


@pytest.fixture
def le_change_test_repo():
    """Points to the le-change-test repo (skips if not populated)."""
    path = "/Users/gatema/Desktop/drive/git/code/le-change-test"
    if not os.path.isdir(os.path.join(path, ".git")):
        pytest.skip("le-change-test repo not available")
    # Verify it has commits
    result = subprocess.run(
        ["git", "log", "--oneline", "-1"],
        cwd=path,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        pytest.skip("le-change-test repo has no commits")
    return path


@pytest.fixture
def le_change_test_shas(le_change_test_repo):
    """Return dict of commit SHAs from le-change-test repo."""
    result = subprocess.run(
        ["git", "log", "--format=%H %s", "--reverse", "main"],
        cwd=le_change_test_repo,
        capture_output=True,
        text=True,
        check=True,
    )
    shas = {}
    for i, line in enumerate(result.stdout.strip().split("\n")):
        sha, msg = line.split(" ", 1)
        shas[f"commit{i + 1}"] = sha
        shas[msg] = sha
    return shas


def get_head_sha(repo_path):
    """Get the HEAD SHA of a git repo."""
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=repo_path,
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout.strip()


def get_prev_sha(repo_path):
    """Get HEAD^ SHA of a git repo."""
    result = subprocess.run(
        ["git", "rev-parse", "HEAD~1"],
        cwd=repo_path,
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout.strip()
