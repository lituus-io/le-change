"""Tests for exception hierarchy."""

from lechange import (
    LeChangeError,
    GitError,
    ConfigError,
    PathError,
    LeChangeRuntimeError,
    RecoveryError,
    YamlError,
    ShallowCloneError,
)


class TestExceptionHierarchy:
    def test_lechange_error_subclasses_exception(self):
        assert issubclass(LeChangeError, Exception)

    def test_git_error_subclasses_lechange_error(self):
        assert issubclass(GitError, LeChangeError)

    def test_config_error_subclasses_lechange_error(self):
        assert issubclass(ConfigError, LeChangeError)

    def test_path_error_subclasses_lechange_error(self):
        assert issubclass(PathError, LeChangeError)

    def test_runtime_error_subclasses_lechange_error(self):
        assert issubclass(LeChangeRuntimeError, LeChangeError)

    def test_recovery_error_subclasses_lechange_error(self):
        assert issubclass(RecoveryError, LeChangeError)

    def test_yaml_error_subclasses_lechange_error(self):
        assert issubclass(YamlError, LeChangeError)

    def test_shallow_clone_error_subclasses_lechange_error(self):
        assert issubclass(ShallowCloneError, LeChangeError)


class TestExceptionCatching:
    def test_catch_as_base(self):
        try:
            raise ConfigError("test message")
        except LeChangeError as e:
            assert "test message" in str(e)

    def test_catch_as_specific(self):
        try:
            raise PathError("bad path")
        except PathError as e:
            assert "bad path" in str(e)

    def test_message_preserved(self):
        try:
            raise RecoveryError("recovery failed")
        except RecoveryError as e:
            assert "recovery failed" in str(e)
