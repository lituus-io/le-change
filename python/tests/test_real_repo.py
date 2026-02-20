"""Tests using the real le-change-test repo."""

import pytest
from lechange import ChangeDetector, Config

pytestmark = pytest.mark.real_repo


class TestDetectAllChanges:
    def test_detect_all_changes(self, le_change_test_repo, le_change_test_shas):
        """Compare first..last commit, verify multiple change types."""
        detector = ChangeDetector(le_change_test_repo)
        config = Config(
            base_sha=le_change_test_shas["commit1"],
            sha=le_change_test_shas["commit5"],
        )
        result = detector.get_changed_files(config)
        all_files = list(result.all_changed_files)
        assert len(all_files) > 0
        # Should have additions, deletions, and renames across all commits
        assert result.any_changed


class TestDetectAdditionsOnly:
    def test_additions_commit1_to_2(self, le_change_test_repo, le_change_test_shas):
        """Commit 1→2 should only add files."""
        detector = ChangeDetector(le_change_test_repo)
        config = Config(
            base_sha=le_change_test_shas["commit1"],
            sha=le_change_test_shas["commit2"],
        )
        result = detector.get_changed_files(config)
        added = list(result.added_files)
        assert len(added) > 0
        # Should include helpers.ts and validators.ts and settings.yaml
        paths = set(added)
        assert any("helpers" in p for p in paths)
        assert any("validators" in p for p in paths)


class TestDetectRenames:
    def test_renames_commit2_to_3(self, le_change_test_repo, le_change_test_shas):
        """Commit 2→3 renames routes.ts to router.ts."""
        detector = ChangeDetector(le_change_test_repo)
        config = Config(
            base_sha=le_change_test_shas["commit2"],
            sha=le_change_test_shas["commit3"],
        )
        result = detector.get_changed_files(config)
        # Should detect rename or changes
        all_files = list(result.all_changed_files)
        assert len(all_files) > 0


class TestDetectDeletions:
    def test_deletions_commit3_to_4(self, le_change_test_repo, le_change_test_shas):
        """Commit 3→4 deletes docs/README.md and src/utils/validators.ts."""
        detector = ChangeDetector(le_change_test_repo)
        config = Config(
            base_sha=le_change_test_shas["commit3"],
            sha=le_change_test_shas["commit4"],
        )
        result = detector.get_changed_files(config)
        deleted = list(result.deleted_files)
        assert len(deleted) > 0
        assert any("README" in p for p in deleted) or any("validators" in p for p in deleted)


class TestPatternFilterTsx:
    def test_filter_tsx(self, le_change_test_repo, le_change_test_shas):
        """Filter for *.tsx files only."""
        detector = ChangeDetector(le_change_test_repo)
        config = Config(
            base_sha=le_change_test_shas["commit1"],
            sha=le_change_test_shas["commit5"],
            files=["**/*.tsx"],
        )
        result = detector.get_changed_files(config)
        for f in result.all_changed_files:
            assert f.endswith(".tsx"), f"Expected .tsx file, got {f}"


class TestPatternFilterYamlGroups:
    def test_yaml_groups(self, le_change_test_repo, le_change_test_shas):
        """Use YAML groups for frontend/backend."""
        yaml = """
frontend:
  - "src/components/**"
backend:
  - "src/api/**"
"""
        detector = ChangeDetector(le_change_test_repo)
        config = Config(
            base_sha=le_change_test_shas["commit1"],
            sha=le_change_test_shas["commit5"],
            files_yaml=yaml,
        )
        result = detector.get_changed_files(config)
        keys = list(result.changed_keys)
        # Should have detected changes in frontend and/or backend groups
        assert len(keys) > 0


class TestDirNames:
    def test_dir_names(self, le_change_test_repo, le_change_test_shas):
        """Config dir_names=True extracts directory names."""
        detector = ChangeDetector(le_change_test_repo)
        config = Config(
            base_sha=le_change_test_shas["commit1"],
            sha=le_change_test_shas["commit5"],
            dir_names=True,
        )
        result = detector.get_changed_files(config)
        all_files = list(result.all_changed_files)
        # With dir_names, results should be directory paths
        assert len(all_files) > 0


class TestRenameSplitting:
    def test_rename_as_delete_add(self, le_change_test_repo, le_change_test_shas):
        """Config output_renamed_as_deleted_added=True splits renames."""
        detector = ChangeDetector(le_change_test_repo)
        config = Config(
            base_sha=le_change_test_shas["commit2"],
            sha=le_change_test_shas["commit3"],
            output_renamed_as_deleted_added=True,
        )
        result = detector.get_changed_files(config)
        # Should have files in added and/or deleted
        all_files = list(result.all_changed_files)
        assert len(all_files) > 0


class TestCountsConsistent:
    def test_counts_consistent(self, le_change_test_repo, le_change_test_shas):
        """Per-type counts should be consistent."""
        detector = ChangeDetector(le_change_test_repo)
        config = Config(
            base_sha=le_change_test_shas["commit1"],
            sha=le_change_test_shas["commit5"],
        )
        result = detector.get_changed_files(config)
        assert result.added_files_count == len(list(result.added_files))
        assert result.deleted_files_count == len(list(result.deleted_files))
        assert result.modified_files_count == len(list(result.modified_files))
        assert result.renamed_files_count == len(list(result.renamed_files))


class TestPosixPaths:
    def test_posix_paths(self, le_change_test_repo, le_change_test_shas):
        """Config use_posix_path_separator=True produces forward slashes."""
        detector = ChangeDetector(le_change_test_repo)
        config = Config(
            base_sha=le_change_test_shas["commit1"],
            sha=le_change_test_shas["commit5"],
            use_posix_path_separator=True,
        )
        result = detector.get_changed_files(config)
        for f in result.all_changed_files:
            assert "\\" not in f, f"Expected POSIX path, got {f}"
