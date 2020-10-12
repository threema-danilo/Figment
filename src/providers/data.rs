use std::marker::PhantomData;
use std::io::{Read, BufReader};
use std::path::{Path, PathBuf};

use serde::de::{self, DeserializeOwned};

use crate::value::{Map, Dict};
use crate::{Error, Profile, Provider, Metadata};

#[derive(Debug, Clone)]
enum Source {
    File(Option<PathBuf>),
    String(String)
}

/// A `Provider` that sources values from a file or string in a given
/// [`Format`].
///
/// # Constructing
///
/// A `Data` provider is typically constructed indirectly via a type that
/// implements the [`Format`] trait via the [`Format::file()`] and
/// [`Format::string()`] methods which in-turn defer to [`Data::file()`] and
/// [`Data::string()`] by default:
///
/// ```rust
/// // The `Format` trait must be in-scope to use its methods.
/// use figment::providers::{Format, Data, Json};
///
/// // These two are equivalaent, except the former requires the explicit type.
/// let json = Data::<Json>::file("foo.json");
/// let json = Json::file("foo.json");
/// ```
///
/// # Provider Details
///
///   * **Profile**
///
///     This provider does not set a profile.
///
///   * **Metadata**
///
///     This provider is named `${NAME} file` (when constructed via
///     [`Data::file()`]) or `${NAME} source string` (when constructed via
///     [`Data::string()`]), where `${NAME}` is [`Format::NAME`]. When
///     constructed from a file, the file's path is specified as file
///     [`Source`](crate::Source). Path interpolation is unchanged from the
///     default.
///
///   * **Data (Unnested, _default_)**
///
///     When nesting is _not_ specified, the source file or string is read and
///     parsed, and the parsed dictionary is emitted into the profile
///     configurable via [`Data::profile()`], which defaults to
///     [`Profile::Default`]. If the source is a file and the file is not
///     present, an empty dictionary is emitted.
///
///   * **Data (Nested)**
///
///     When nesting is specified, the source value is exepcted to be a
///     dictionary. It's top-level keys are emitted as profiles, and the value
///     corresponding to each key as the profile data.
#[derive(Debug, Clone)]
pub struct Data<F: Format> {
    source: Source,
    /// The profile data will be emitted to if nesting is disabled. Defaults to
    /// [`Profile::Default`].
    pub profile: Option<Profile>,
    _format: PhantomData<F>,
}

impl<F: Format> Data<F> {
    fn new(source: Source, profile: Option<Profile>) -> Self {
        Data { source, profile, _format: PhantomData }
    }

    /// Returns a `Data` provider that sources its values by parsing the file at
    /// `path` as format `F`. If `path` is relative, the file is searched for in
    /// the current working directory and all parent directories until the root,
    /// and the first hit is used.
    ///
    /// Nesting is not enabled by default; use [`Data::nested()`] to enable
    /// nesting.
    ///
    /// ```rust
    /// use serde::Deserialize;
    /// use figment::{Figment, Jail, providers::{Format, Toml}, value::Map};
    ///
    /// #[derive(Debug, PartialEq, Deserialize)]
    /// struct Config {
    ///     numbers: Vec<usize>,
    ///     untyped: Map<String, usize>,
    /// }
    ///
    /// Jail::expect_with(|jail| {
    ///     jail.create_file("Config.toml", r#"
    ///         numbers = [1, 2, 3]
    ///
    ///         [untyped]
    ///         key = 1
    ///         other = 4
    ///     "#)?;
    ///
    ///     let config: Config = Figment::from(Toml::file("Config.toml")).extract()?;
    ///     assert_eq!(config, Config {
    ///         numbers: vec![1, 2, 3],
    ///         untyped: figment::util::map!["key".into() => 1, "other".into() => 4],
    ///     });
    ///
    ///     Ok(())
    /// });
    /// ```
    pub fn file<P: AsRef<Path>>(path: P) -> Self {
        fn find(path: &Path) -> Option<PathBuf> {
            if path.is_absolute() {
                match path.is_file() {
                    true => return Some(path.to_path_buf()),
                    false => return None
                }
            }

            let cwd = std::env::current_dir().ok()?;
            let mut cwd = cwd.as_path();
            loop {
                let file_path = cwd.join(path);
                if file_path.is_file() {
                    return Some(file_path);
                }

                cwd = cwd.parent()?;
            }
        }

        Data::new(Source::File(find(path.as_ref())), Some(Profile::Default))
    }

    /// Returns a `Data` provider that sources its values by parsing the string
    /// `string` as format `F`. Nesting is not enabled by default; use
    /// [`Data::nested()`] to enable nesting.
    ///
    /// ```rust
    /// use serde::Deserialize;
    /// use figment::{Figment, Jail, providers::{Format, Toml}, value::Map};
    ///
    /// #[derive(Debug, PartialEq, Deserialize)]
    /// struct Config {
    ///     numbers: Vec<usize>,
    ///     untyped: Map<String, usize>,
    /// }
    ///
    /// Jail::expect_with(|jail| {
    ///     let source = r#"
    ///         numbers = [1, 2, 3]
    ///         untyped = { key = 1, other = 4 }
    ///     "#;
    ///
    ///     let config: Config = Figment::from(Toml::string(source)).extract()?;
    ///     assert_eq!(config, Config {
    ///         numbers: vec![1, 2, 3],
    ///         untyped: figment::util::map!["key".into() => 1, "other".into() => 4],
    ///     });
    ///
    ///     Ok(())
    /// });
    /// ```
    pub fn string(string: &str) -> Self {
        Data::new(Source::String(string.into()), Some(Profile::Default))
    }

    /// Enables nesting on `self`, which results in top-level keys of the
    /// sourced data being treated as profiles.
    ///
    /// ```rust
    /// use serde::Deserialize;
    /// use figment::{Figment, Jail, providers::{Format, Toml}, value::Map};
    ///
    /// #[derive(Debug, PartialEq, Deserialize)]
    /// struct Config {
    ///     numbers: Vec<usize>,
    ///     untyped: Map<String, usize>,
    /// }
    ///
    /// Jail::expect_with(|jail| {
    ///     jail.create_file("Config.toml", r#"
    ///         [global.untyped]
    ///         global = 0
    ///         hi = 7
    ///
    ///         [staging]
    ///         numbers = [1, 2, 3]
    ///
    ///         [release]
    ///         numbers = [6, 7, 8]
    ///     "#)?;
    ///
    ///     // Enable nesting via `nested()`.
    ///     let figment = Figment::from(Toml::file("Config.toml").nested());
    ///
    ///     let figment = figment.select("staging");
    ///     let config: Config = figment.extract()?;
    ///     assert_eq!(config, Config {
    ///         numbers: vec![1, 2, 3],
    ///         untyped: figment::util::map!["global".into() => 0, "hi".into() => 7],
    ///     });
    ///
    ///     let config: Config = figment.select("release").extract()?;
    ///     assert_eq!(config, Config {
    ///         numbers: vec![6, 7, 8],
    ///         untyped: figment::util::map!["global".into() => 0, "hi".into() => 7],
    ///     });
    ///
    ///     Ok(())
    /// });
    /// ```
    pub fn nested(mut self) -> Self {
        self.profile = None;
        self
    }

    /// Set the profile to emit data to when nesting is disabled.
    ///
    /// ```rust
    /// use serde::Deserialize;
    /// use figment::{Figment, Jail, providers::{Format, Toml}, value::Map};
    ///
    /// #[derive(Debug, PartialEq, Deserialize)]
    /// struct Config { value: u8 }
    ///
    /// Jail::expect_with(|jail| {
    ///     let provider = Toml::string("value = 123").profile("debug");
    ///     let config: Config = Figment::from(provider).select("debug").extract()?;
    ///     assert_eq!(config, Config { value: 123 });
    ///
    ///     Ok(())
    /// });
    /// ```
    pub fn profile<P: Into<Profile>>(mut self, profile: P) -> Self {
        self.profile = Some(profile.into());
        self
    }
}

impl<F: Format> Provider for Data<F> {
    fn metadata(&self) -> Metadata {
        use Source::*;
        match &self.source {
            String(_) => Metadata::named(format!("{} source string", F::NAME)),
            File(None) => Metadata::named(format!("{} file", F::NAME)),
            File(Some(p)) => Metadata::from(format!("{} file", F::NAME), &**p)
        }
    }

    fn data(&self) -> Result<Map<Profile, Dict>, Error> {
        use Source::*;
        let map: Result<Map<Profile, Dict>, _> = match (&self.source, &self.profile) {
            (File(None), _) => return Ok(Map::new()),
            (File(Some(path)), None) => F::from_path(&path),
            (String(s), None) => F::from_str(&s),
            (File(Some(path)), Some(prof)) => F::from_path(&path).map(|v| prof.collect(v)),
            (String(s), Some(prof)) => F::from_str(&s).map(|v| prof.collect(v)),
        };

        Ok(map.map_err(|e| e.to_string())?)
    }
}

/// Trait implementable by text-based [`Data`] format providers.
///
/// Instead of implementing [`Provider`] directly, types that refer to data
/// formats, such as [`Json`] and [`Toml`], implement this trait. By
/// implementing [`Format`], they become [`Provider`]s indirectly via the
/// [`Data`] type, which serves as a provider for all `T: Format`.
///
/// # Implementing
///
/// There are two primary implementation items to consider:
///
///   1. [`NAME`]: This should be the name of the data format: `"JSON"` or
///      `"TOML"`. The string is used in the [metadata for
///      `Data`](Data#provider-details).
///
///   2. [`from_str()`]: This is the core string deserialization method, for
///      instance, [`toml::from_str`]. For writing a custom data format, see
///      [serde's writing a data format guide](https://serde.rs/data-format.html).
///
/// [`NAME`]: Format::NAME
/// [`from_str()`]: Format::from_str
pub trait Format: Sized {
    /// The data format's error type.
    type Error: de::Error;

    /// The name of the data format, for instance `"JSON"` or `"TOML"`.
    const NAME: &'static str;

    /// Parses `string` as the data format `Self` as a `T` or returns an error
    /// if the `string` is an invalid `T`.
    fn from_str<'de, T: DeserializeOwned>(string: &'de str) -> Result<T, Self::Error>;

    /// Parses the file at `path` as the data format `Self` as a `T` or returns
    /// an error if the `string` is an invalid `T`. The default implementation
    /// calls [`Format::from_str()`] with the contents of the file.
    fn from_path<T: DeserializeOwned>(path: &Path) -> Result<T, Self::Error> {
        let mut source = String::new();
        std::fs::File::open(path)
            .map(|file| BufReader::new(file))
            .and_then(|mut reader| reader.read_to_string(&mut source))
            .map_err(|e| de::Error::custom(e))?;

        Self::from_str(&source)
    }

    /// Returns a `Data` provider that sources its values by parsing the file at
    /// `path` as format `Self`. See [`Data::file()`] for more details. The
    /// default implementation calls `Data::file(path)`.
    fn file<P: AsRef<Path>>(path: P) -> Data<Self> {
        Data::file(path)
    }

    /// Returns a `Data` provider that sources its values by parsing `string` as
    /// format `Self`. See [`Data::string()`] for more details. The default
    /// implementation calls `Data::string(string)`.
    fn string(string: &str) -> Data<Self> {
        Data::string(string)
    }
}

#[allow(unused_macros)]
macro_rules! impl_format {
    ($name:ident $NAME:literal/$string:literal: $func:path, $E:ty, $doc:expr) => (
        #[cfg(feature = $string)]
        #[cfg_attr(nightly, doc(cfg(feature = $string)))]
        #[doc = $doc]
        pub struct $name;

        #[cfg(feature = $string)]
        impl Format for $name {
            type Error = $E;

            const NAME: &'static str = $NAME;

            fn from_str<'de, T: DeserializeOwned>(s: &'de str) -> Result<T, $E> {
                $func(s)
            }
        }
    );

    ($name:ident $NAME:literal/$string:literal: $func:path, $E:ty) => (
        impl_format!($name $NAME/$string: $func, $E, concat!(
            "A ", $NAME, " [`Format`] [`Data`] provider. See [`Data`] for details.",
            "\n```\n",
            "use figment::providers::{Format, ", stringify!($name), "};",
            "\n\n// Source directly from a source string...",
            "\nlet provider = ", stringify!($name), r#"::string("source-string");"#,
            "\n\n// Or read from a file on disk.",
            "\nlet provider = ", stringify!($name), r#"::file("path-to-file");"#,
            "\n```",
            "\n\nSee also [`", stringify!($func), "`] for parsing details."
        ));
    )
}

impl_format!(Toml "TOML"/"toml": toml::from_str, toml::de::Error);
impl_format!(Yaml "YAML"/"yaml": serde_yaml::from_str, serde_yaml::Error);
impl_format!(Json "JSON"/"json": serde_json::from_str, serde_json::error::Error);
