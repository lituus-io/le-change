//! Error handling for Python bindings

use pyo3::exceptions::PyException;
use pyo3::prelude::*;

pyo3::create_exception!(lechange, LeChangeError, PyException);
pyo3::create_exception!(lechange, ConfigError, LeChangeError);
pyo3::create_exception!(lechange, GitError, LeChangeError);
pyo3::create_exception!(lechange, PathError, LeChangeError);
pyo3::create_exception!(lechange, RuntimeError, LeChangeError);

/// Register custom exceptions with Python module
pub fn register_exceptions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("LeChangeError", module.py().get_type::<LeChangeError>())?;
    module.add("ConfigError", module.py().get_type::<ConfigError>())?;
    module.add("GitError", module.py().get_type::<GitError>())?;
    module.add("PathError", module.py().get_type::<PathError>())?;
    module.add("RuntimeError", module.py().get_type::<RuntimeError>())?;
    Ok(())
}
