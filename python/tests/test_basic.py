"""
Basic tests for LeChange Python bindings.

Copyright (c) 2024-2026 lituus-io
Author: terekete <spicyzhug@gmail.com>
License: MIT
"""

import pytest
from lechange import ChangeDetector, Config, ChangedFiles


def test_import():
    """Test that the module can be imported."""
    assert ChangeDetector is not None
    assert Config is not None
    assert ChangedFiles is not None


def test_version():
    """Test that version is available."""
    import lechange
    assert hasattr(lechange, "__version__")
    assert isinstance(lechange.__version__, str)


def test_config_creation():
    """Test Config creation with default values."""
    config = Config()
    assert config is not None


def test_config_with_parameters():
    """Test Config creation with parameters."""
    config = Config(
        base_sha="main",
        sha="HEAD",
        files=["**/*.py"],
        json=True,
    )
    assert config is not None


def test_detector_creation():
    """Test ChangeDetector creation."""
    detector = ChangeDetector(".")
    assert detector is not None


def test_detector_repr():
    """Test ChangeDetector repr."""
    detector = ChangeDetector(".")
    r = repr(detector)
    assert "ChangeDetector" in r


def test_config_repr():
    """Test Config repr."""
    config = Config()
    r = repr(config)
    assert "Config" in r
