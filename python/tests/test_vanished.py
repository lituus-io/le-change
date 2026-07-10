"""Vanished-file detection (detect_vanished) through the Python bindings."""

import subprocess

import pytest
from lechange import ChangeDetector, Config


def _git(repo, *args):
    subprocess.run(
        ["git", "-c", "user.email=t@t", "-c", "user.name=t", *args],
        cwd=repo,
        check=True,
        capture_output=True,
    )


def _rev(repo):
    return subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=repo,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


@pytest.fixture
def vanished_repo(tmp_path):
    """base -> add stacks/gone/Pulumi.yaml -> remove it again."""
    repo = tmp_path / "repo"
    repo.mkdir()
    _git(repo, "init", "-q", "-b", "main")
    (repo / "README.md").write_text("r")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "base")
    base = _rev(repo)

    stack = repo / "stacks" / "gone"
    stack.mkdir(parents=True)
    (stack / "Pulumi.yaml").write_text("name: gone")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "add stack")
    add_sha = _rev(repo)

    _git(repo, "rm", "-rq", "stacks/gone")
    _git(repo, "commit", "-qm", "remove stack")
    head = _rev(repo)
    return repo, base, add_sha, head


def test_config_vanished_fields_roundtrip():
    config = Config(
        detect_vanished=True,
        vanished_max_commits=42,
        deleted_to_destroy=True,
    )
    assert config is not None  # constructor accepts the new fields


def test_vanished_detection_and_destroy_decision(vanished_repo):
    repo, base, add_sha, head = vanished_repo
    detector = ChangeDetector(str(repo))
    config = Config(
        base_sha=base,
        sha=head,
        files_group_by="stacks/{group}/**",
        detect_vanished=True,
    )
    result = detector.get_changed_files(config)

    assert result.any_vanished
    assert result.vanished_files_count == 1
    assert result.vanished_files == ["stacks/gone/Pulumi.yaml"]
    entry = result.vanished[0]
    assert entry["path"] == "stacks/gone/Pulumi.yaml"
    assert entry["last_seen_sha"] == add_sha

    destroys = [d for d in result.deploy_decisions if d["action"] == "destroy"]
    assert len(destroys) == 1
    assert destroys[0]["key"] == "gone"
    assert destroys[0]["reason"] == "vanished"
    assert destroys[0]["last_seen_sha"] == add_sha
    assert '"action":"destroy"' in result.deploy_matrix


def test_vanished_off_by_default(vanished_repo):
    repo, base, _add_sha, head = vanished_repo
    detector = ChangeDetector(str(repo))
    config = Config(base_sha=base, sha=head, files_group_by="stacks/{group}/**")
    result = detector.get_changed_files(config)

    assert not result.any_vanished
    assert result.vanished_files == []
    assert all(d["action"] != "destroy" for d in result.deploy_decisions)
