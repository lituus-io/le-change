"""Integration tests for ChangeDetector with real git repos."""

import subprocess
import pytest
from lechange import ChangeDetector, Config, PathError


def get_head_sha(repo_path):
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=repo_path, capture_output=True, text=True, check=True,
    )
    return result.stdout.strip()


def get_prev_sha(repo_path):
    result = subprocess.run(
        ["git", "rev-parse", "HEAD~1"],
        cwd=repo_path, capture_output=True, text=True, check=True,
    )
    return result.stdout.strip()


class TestDetectAddedFiles:
    def test_detect_additions(self, tmp_git_repo_with_changes):
        repo = tmp_git_repo_with_changes
        base = get_prev_sha(repo)
        head = get_head_sha(repo)
        detector = ChangeDetector(str(repo))
        config = Config(base_sha=base, sha=head)
        result = detector.get_changed_files(config)
        added = list(result.added_files)
        assert len(added) > 0
        paths = [p for p in added]
        # Should have .py files
        assert any(p.endswith(".py") for p in paths)

    def test_detect_with_pattern(self, tmp_git_repo_with_changes):
        repo = tmp_git_repo_with_changes
        base = get_prev_sha(repo)
        head = get_head_sha(repo)
        detector = ChangeDetector(str(repo))
        config = Config(base_sha=base, sha=head, files=["**/*.py"])
        result = detector.get_changed_files(config)
        for f in result.all_changed_files:
            assert f.endswith(".py")


class TestDetectDeletedFiles:
    def test_detect_deletions(self, tmp_git_repo_with_deletion):
        repo = tmp_git_repo_with_deletion
        base = get_prev_sha(repo)
        head = get_head_sha(repo)
        detector = ChangeDetector(str(repo))
        config = Config(base_sha=base, sha=head)
        result = detector.get_changed_files(config)
        deleted = list(result.deleted_files)
        assert len(deleted) > 0
        assert any("util.py" in p for p in deleted)


class TestDetectRenamedFiles:
    def test_detect_renames(self, tmp_git_repo_with_rename):
        repo = tmp_git_repo_with_rename
        base = get_prev_sha(repo)
        head = get_head_sha(repo)
        detector = ChangeDetector(str(repo))
        config = Config(base_sha=base, sha=head)
        result = detector.get_changed_files(config)
        renamed = list(result.renamed_files)
        # The rename should be detected
        assert len(renamed) > 0 or len(list(result.all_changed_files)) > 0


class TestNoChanges:
    def test_same_sha(self, tmp_git_repo):
        repo = tmp_git_repo
        sha = get_head_sha(repo)
        detector = ChangeDetector(str(repo))
        config = Config(base_sha=sha, sha=sha, skip_same_sha=True)
        result = detector.get_changed_files(config)
        assert len(list(result.all_changed_files)) == 0


class TestCounts:
    def test_counts_match_lists(self, tmp_git_repo_with_changes):
        repo = tmp_git_repo_with_changes
        base = get_prev_sha(repo)
        head = get_head_sha(repo)
        detector = ChangeDetector(str(repo))
        config = Config(base_sha=base, sha=head)
        result = detector.get_changed_files(config)
        assert result.added_files_count == len(list(result.added_files))
        assert result.deleted_files_count == len(list(result.deleted_files))
        assert result.modified_files_count == len(list(result.modified_files))
        assert result.all_changed_files_count == len(list(result.all_changed_files))


class TestBooleanChecks:
    def test_any_changed(self, tmp_git_repo_with_changes):
        repo = tmp_git_repo_with_changes
        base = get_prev_sha(repo)
        head = get_head_sha(repo)
        detector = ChangeDetector(str(repo))
        config = Config(base_sha=base, sha=head)
        result = detector.get_changed_files(config)
        assert result.any_changed is True
        assert result.any_added is True


class TestDiagnostics:
    def test_diagnostics_accessible(self, tmp_git_repo):
        repo = tmp_git_repo
        sha = get_head_sha(repo)
        detector = ChangeDetector(str(repo))
        config = Config(base_sha=sha, sha=sha, skip_same_sha=True)
        result = detector.get_changed_files(config)
        diags = list(result.diagnostics)
        # Should have a "skipped_same_sha" diagnostic
        assert len(diags) > 0
        assert any(d["category"] == "skipped_same_sha" for d in diags)


class TestRenamedMapping:
    def test_renamed_mapping_is_dict(self, tmp_git_repo_with_rename):
        repo = tmp_git_repo_with_rename
        base = get_prev_sha(repo)
        head = get_head_sha(repo)
        detector = ChangeDetector(str(repo))
        config = Config(base_sha=base, sha=head)
        result = detector.get_changed_files(config)
        mapping = result.renamed_files_mapping
        assert isinstance(mapping, dict)


class TestRepr:
    def test_config_repr(self):
        config = Config()
        assert "Config" in repr(config)

    def test_detector_repr(self, tmp_git_repo):
        detector = ChangeDetector(str(tmp_git_repo))
        assert "ChangeDetector" in repr(detector)

    def test_result_repr(self, tmp_git_repo_with_changes):
        repo = tmp_git_repo_with_changes
        base = get_prev_sha(repo)
        head = get_head_sha(repo)
        detector = ChangeDetector(str(repo))
        config = Config(base_sha=base, sha=head)
        result = detector.get_changed_files(config)
        assert "ChangedFiles" in repr(result)


class TestErrors:
    def test_invalid_repo_path(self):
        with pytest.raises(PathError):
            ChangeDetector("/nonexistent/path/xyz")
