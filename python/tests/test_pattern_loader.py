"""Tests for load_yaml_patterns function."""

import pytest
from lechange import load_yaml_patterns, YamlError


class TestLoadYamlPatterns:
    def test_basic_groups(self):
        yaml = """
frontend:
  - "src/components/**"
  - "src/pages/**"
backend:
  - "src/api/**"
"""
        groups = load_yaml_patterns(yaml)
        assert len(groups) == 2
        names = {g["name"] for g in groups}
        assert "frontend" in names
        assert "backend" in names

    def test_matcher_works(self):
        yaml = """
frontend:
  - "src/components/**"
"""
        groups = load_yaml_patterns(yaml)
        assert len(groups) == 1
        matcher = groups[0]["matcher"]
        assert matcher.matches("src/components/Button.tsx")
        assert not matcher.matches("src/api/routes.ts")

    def test_exclude_patterns(self):
        yaml = """
frontend:
  - "src/components/**"
  - "!src/components/test/**"
"""
        groups = load_yaml_patterns(yaml, negation_first=True)
        matcher = groups[0]["matcher"]
        assert matcher.matches("src/components/Button.tsx")
        assert not matcher.matches("src/components/test/Button.test.tsx")

    def test_negation_first(self):
        yaml = """
group:
  - "**/*.ts"
  - "!**/test_*"
"""
        groups = load_yaml_patterns(yaml, negation_first=True)
        matcher = groups[0]["matcher"]
        assert matcher.matches("src/main.ts")
        assert not matcher.matches("src/test_main.ts")

    def test_invalid_yaml(self):
        with pytest.raises(YamlError):
            load_yaml_patterns("not: [valid: yaml")
