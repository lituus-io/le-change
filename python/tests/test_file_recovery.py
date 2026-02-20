"""Tests for FileRecovery Python bindings."""

import subprocess
import pytest
from lechange import FileRecovery, PathError, RecoveryError


def get_head_sha(repo_path):
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=repo_path, capture_output=True, text=True, check=True,
    )
    return result.stdout.strip()


class TestConstruction:
    def test_valid_path(self, tmp_git_repo):
        fr = FileRecovery(str(tmp_git_repo))
        assert fr is not None

    def test_invalid_path(self):
        with pytest.raises(PathError):
            FileRecovery("/nonexistent/path/xyz")

    def test_repr(self, tmp_git_repo):
        fr = FileRecovery(str(tmp_git_repo))
        assert "FileRecovery" in repr(fr)


class TestRecoverFile:
    def test_recover_existing(self, tmp_git_repo, tmp_path):
        sha = get_head_sha(tmp_git_repo)
        fr = FileRecovery(str(tmp_git_repo))
        output = str(tmp_path / "output")
        import os
        os.makedirs(output, exist_ok=True)
        result = fr.recover_file(sha, "init.txt", output)
        assert "init.txt" in result

    def test_nonexistent_file(self, tmp_git_repo, tmp_path):
        sha = get_head_sha(tmp_git_repo)
        fr = FileRecovery(str(tmp_git_repo))
        output = str(tmp_path / "output")
        import os
        os.makedirs(output, exist_ok=True)
        with pytest.raises(RecoveryError):
            fr.recover_file(sha, "nonexistent.txt", output)

    def test_invalid_sha(self, tmp_git_repo, tmp_path):
        fr = FileRecovery(str(tmp_git_repo))
        output = str(tmp_path / "output")
        import os
        os.makedirs(output, exist_ok=True)
        with pytest.raises(RecoveryError):
            fr.recover_file("invalid_sha", "init.txt", output)
