use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Availability<T> {
    Present(T),
    Absent(Reason),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reason {
    Loading,
    Error(String),
    Corrupted, // Fix 4: Disk Race
               // Can be extended with more reasons
}

impl<T> Availability<T> {
    pub fn map<U, F>(self, f: F) -> Availability<U>
    where
        F: FnOnce(T) -> U,
    {
        match self {
            Availability::Present(v) => Availability::Present(f(v)),
            Availability::Absent(r) => Availability::Absent(r),
        }
    }

    pub fn and_then<U, F>(self, f: F) -> Availability<U>
    where
        F: FnOnce(T) -> Availability<U>,
    {
        match self {
            Availability::Present(v) => f(v),
            Availability::Absent(r) => Availability::Absent(r),
        }
    }

    pub fn unwrap(self) -> T {
        match self {
            Availability::Present(v) => v,
            Availability::Absent(r) => panic!("Called unwrap on Absent value: {:?}", r),
        }
    }

    pub fn unwrap_or(self, default: T) -> T {
        match self {
            Availability::Present(v) => v,
            Availability::Absent(_) => default,
        }
    }
}

// Implement From<T> for Availability<T> for convenience
impl<T> From<T> for Availability<T> {
    fn from(value: T) -> Self {
        Availability::Present(value)
    }
}

/// Macro to unwrap Availability or return Absent early.
/// Acts like the `?` operator for Availability.
#[macro_export]
macro_rules! available {
    ($e:expr) => {
        match $e {
            $crate::Availability::Present(v) => v,
            $crate::Availability::Absent(r) => return $crate::Availability::Absent(r),
        }
    };
}
