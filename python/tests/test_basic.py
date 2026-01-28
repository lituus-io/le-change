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
        base="main",
        head="HEAD",
        files=["**/*.py"],
        json=True
    )
    assert config is not None


def test_detector_creation():
    """Test ChangeDetector creation."""
    detector = ChangeDetector(".")
    assert detector is not None


@pytest.mark.skipif(True, reason="Requires git repository setup")
def test_basic_detection():
    """Test basic change detection (requires git repo)."""
    detector = ChangeDetector(".")
    config = Config(base="HEAD^", head="HEAD")
    result = detector.get_changed_files(config)

    assert result is not None
    assert hasattr(result, "all_changed_files")
    assert hasattr(result, "any_changed")


@pytest.mark.asyncio
@pytest.mark.skipif(True, reason="Requires git repository setup")
async def test_async_detection():
    """Test async change detection (requires git repo)."""
    detector = ChangeDetector(".")
    config = Config(base="HEAD^", head="HEAD")
    result = await detector.get_changed_files_async(config)

    assert result is not None
