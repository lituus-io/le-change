"""Tests for PatternMatcher Python bindings."""

import pytest
from lechange import PatternMatcher, ConfigError


class TestPatternMatcherConstruction:
    def test_default(self):
        m = PatternMatcher()
        assert m is not None

    def test_with_includes(self):
        m = PatternMatcher(includes=["**/*.py"])
        assert m.matches("src/main.py")

    def test_with_excludes(self):
        m = PatternMatcher(excludes=["**/test_*"])
        assert not m.matches("tests/test_main.py")
        assert m.matches("src/main.py")

    def test_with_includes_and_excludes(self):
        m = PatternMatcher(includes=["**/*.py"], excludes=["**/test_*"])
        assert m.matches("src/main.py")
        assert not m.matches("tests/test_main.py")

    def test_negation_first(self):
        m = PatternMatcher(includes=["**/*.py"], excludes=["**/test_*"], negation_first=True)
        # negation_first: check exclude first, then include
        assert m.matches("src/main.py")
        assert not m.matches("tests/test_main.py")

    def test_invalid_pattern_raises_config_error(self):
        with pytest.raises(ConfigError):
            PatternMatcher(includes=["[invalid"])


class TestPatternMatcherMatches:
    def test_match(self):
        m = PatternMatcher(includes=["**/*.py"])
        assert m.matches("src/main.py") is True

    def test_no_match(self):
        m = PatternMatcher(includes=["**/*.py"])
        assert m.matches("src/main.rs") is False

    def test_nested_paths(self):
        m = PatternMatcher(includes=["src/**/*.ts"])
        assert m.matches("src/api/routes.ts")
        assert m.matches("src/components/deep/nested/file.ts")
        assert not m.matches("tests/test.ts")

    def test_extension_patterns(self):
        m = PatternMatcher(includes=["**/*.tsx"])
        assert m.matches("src/components/Button.tsx")
        assert not m.matches("src/components/Button.ts")


class TestPatternMatcherFilter:
    def test_basic_filter(self):
        m = PatternMatcher(includes=["**/*.py"])
        result = m.filter(["a.py", "b.rs", "c.py"])
        assert result == ["a.py", "c.py"]

    def test_empty_input(self):
        m = PatternMatcher(includes=["**/*.py"])
        result = m.filter([])
        assert result == []

    def test_no_matches(self):
        m = PatternMatcher(includes=["**/*.py"])
        result = m.filter(["a.rs", "b.go"])
        assert result == []


class TestPatternMatcherPartition:
    def test_basic_partition(self):
        m = PatternMatcher(includes=["**/*.py"])
        matched, unmatched = m.partition(["a.py", "b.rs", "c.py"])
        assert matched == ["a.py", "c.py"]
        assert unmatched == ["b.rs"]

    def test_all_match(self):
        m = PatternMatcher(includes=["**/*.py"])
        matched, unmatched = m.partition(["a.py", "b.py"])
        assert matched == ["a.py", "b.py"]
        assert unmatched == []

    def test_none_match(self):
        m = PatternMatcher(includes=["**/*.py"])
        matched, unmatched = m.partition(["a.rs", "b.go"])
        assert matched == []
        assert unmatched == ["a.rs", "b.go"]


def test_repr():
    m = PatternMatcher(includes=["**/*.py"])
    assert "PatternMatcher" in repr(m)
