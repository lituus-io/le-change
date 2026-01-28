//! Error handling for Python bindings

use pyo3::prelude::*;
use pyo3::exceptions::PyException;

pyo3::create_exception!(lechange, LeChangeError, PyException);
pyo3::create_exception!(lechange, ConfigError, LeChangeError);
pyo3::create_exception!(lechange, GitError, LeChangeError);
pyo3::create_exception!(lechange, PathError, LeChangeError);
pyo3::create_exception!(lechange, RuntimeError, LeChangeError);

/// Register custom exceptions with Python module
pub fn register_exceptions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("LeChangeError", module.py().get_type_bound::<LeChangeError>())?;
    module.add("ConfigError", module.py().get_type_bound::<ConfigError>())?;
    module.add("GitError", module.py().get_type_bound::<GitError>())?;
    module.add("PathError", module.py().get_type_bound::<PathError>())?;
    module.add("RuntimeError", module.py().get_type_bound::<RuntimeError>())?;
    Ok(())
}
