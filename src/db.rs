use std::str::FromStr;

use redb::{Database, Range, ReadableTable, RedbValue, TableDefinition};
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::pkg::{Installed, Local, Package, PackageInfo, Source};

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, RedbValue)]
pub struct DbPackage {
    pub name: String,
    pub version: String,
    pub license: String,
    pub repository: String,
    pub authors: Vec<String>,
    pub description: String,
    pub dependencies: Vec<String>,
    pub build_dependencies: Vec<String>,
    pub provides: String,
    pub url: String,
    pub checksum: String,
    pub build: String,
    pub install: String,
    pub installed: Installed,
    pub added: bool,
}

fn string_to_option(container: String) -> Option<String> {
    if container.is_empty() {
        None
    } else {
        Some(container)
    }
}

fn vec_to_option<T>(container: Vec<T>) -> Option<Vec<T>> {
    if container.is_empty() {
        None
    } else {
        Some(container)
    }
}

impl From<DbPackage> for Package {
    fn from(value: DbPackage) -> Self {
        let DbPackage {
            name,
            version,
            license,
            repository,
            authors,
            description,
            dependencies,
            build_dependencies,
            provides,
            url,
            checksum,
            build,
            install,
            installed,
            added,
        } = value;

        let version = Version::from_str(&version).expect("invalid version forma");
        let repository = string_to_option(repository);
        let authors = vec_to_option(authors);
        let description = string_to_option(description);
        let dependencies = vec_to_option(dependencies);
        let build_dependencies = vec_to_option(build_dependencies);
        let provides = string_to_option(provides);
        let checksum = string_to_option(checksum);
        let build = string_to_option(build);

        Self {
            info: PackageInfo {
                name,
                version,
                license,
                repository,
                authors,
                description,
                dependencies,
                build_dependencies,
                provides,
            },
            source: Source {
                url,
                checksum,
                build,
                install,
            },
            local: Local { installed, added },
        }
    }
}

impl From<Package> for DbPackage {
    fn from(value: Package) -> Self {
        let Package {
            info:
                PackageInfo {
                    name,
                    version,
                    license,
                    repository,
                    authors,
                    description,
                    dependencies,
                    build_dependencies,
                    provides,
                },
            source:
                Source {
                    url,
                    checksum,
                    build,
                    install,
                },
            local: Local { installed, added },
        } = value;

        let version = version.to_string();
        let repository = repository.unwrap_or_default();
        let authors = authors.unwrap_or_default();
        let description = description.unwrap_or_default();
        let dependencies = dependencies.unwrap_or_default();
        let build_dependencies = build_dependencies.unwrap_or_default();
        let provides = provides.unwrap_or_default();
        let checksum = checksum.unwrap_or_default();
        let build = build.unwrap_or_default();

        Self {
            name,
            version,
            license,
            repository,
            authors,
            description,
            dependencies,
            build_dependencies,
            provides,
            url,
            checksum,
            build,
            install,
            installed,
            added,
        }
    }
}

pub trait Db<'a, 'b> {
    type Error;
    type Key<'k>;
    type Value;
    type ExtValue: From<Self::Value> + Into<Self::Value>;
    type Table;
    type Iterator;

    fn init_table(&self, table: Self::Table) -> Result<(), Self::Error>;

    fn get(
        &self,
        table: Self::Table,
        key: Self::Key<'a>,
    ) -> Result<Option<Self::ExtValue>, Self::Error>;
    fn get_iter<I: IntoIterator<Item = Self::Key<'a>>>(
        &self,
        table: Self::Table,
        keys: I,
    ) -> Result<Vec<Option<Self::ExtValue>>, Self::Error>;

    fn set(
        &self,
        table: Self::Table,
        key: Self::Key<'a>,
        value: Self::ExtValue,
    ) -> Result<(), Self::Error>;
    fn set_iter<I, K, V>(&self, table: Self::Table, iter: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = (Self::Key<'a>, Self::ExtValue)>;

    fn remove(
        &self,
        table: Self::Table,
        key: Self::Key<'a>,
    ) -> Result<Option<Self::ExtValue>, Self::Error>;
    fn remove_iter<I: IntoIterator<Item = Self::Key<'a>>>(
        &self,
        table: Self::Table,
        keys: I,
    ) -> Result<Vec<Option<Self::ExtValue>>, Self::Error>;

    fn modify<F>(&self, table: Self::Table, key: Self::Key<'a>, func: F) -> Result<(), Self::Error>
    where
        F: FnOnce(Option<Self::ExtValue>) -> Option<Self::ExtValue>;
}

impl<'a: 'b, 'b> Db<'a, 'b> for Database {
    type Error = redb::Error;
    type Key<'k> = &'k str;
    type Value = DbPackage;
    type ExtValue = Package;
    type Table = TableDefinition<'a, &'static str, DbPackage>;
    type Iterator = Range<'b, Self::Key<'static>, Self::Value>;

    fn init_table(&self, table: Self::Table) -> Result<(), Self::Error> {
        let write_txn = self.begin_write()?;
        {
            write_txn.open_table(table)?;
        }
        write_txn.commit()?;

        Ok(())
    }

    fn get(
        &self,
        table: Self::Table,
        key: Self::Key<'a>,
    ) -> Result<Option<Self::ExtValue>, Self::Error> {
        let read_txn = self.begin_read()?;
        let read_table = read_txn.open_table(table)?;
        Ok(read_table
            .get(key)
            .map(|r| r.map(|o| Into::<Package>::into(o.value())))?)
    }

    fn get_iter<I: IntoIterator<Item = Self::Key<'a>>>(
        &self,
        table: Self::Table,
        keys: I,
    ) -> Result<Vec<Option<Self::ExtValue>>, Self::Error> {
        let read_txn = self.begin_read()?;
        let read_table = read_txn.open_table(table)?;

        let mut values: Vec<Option<Self::ExtValue>> = Vec::new();
        for key in keys {
            values.push(
                read_table
                    .get(key)
                    .map(|r| r.map(|o| Into::<Package>::into(o.value())))?,
            );
        }

        Ok(values)
    }

    fn set(
        &self,
        table: Self::Table,
        key: Self::Key<'a>,
        value: Self::ExtValue,
    ) -> Result<(), Self::Error> {
        let write_txn = self.begin_write()?;
        {
            let mut write_table = write_txn.open_table(table)?;
            write_table.insert(key, Into::<Self::Value>::into(value))?;
        }
        write_txn.commit()?;

        Ok(())
    }

    fn set_iter<I, K, V>(&self, table: Self::Table, iter: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = (Self::Key<'a>, Self::ExtValue)>,
    {
        let write_txn = self.begin_write()?;
        {
            let mut write_table = write_txn.open_table(table)?;
            for (key, value) in iter {
                write_table.insert(key, Into::<Self::Value>::into(value))?;
            }
        }
        write_txn.commit()?;

        Ok(())
    }

    fn remove(
        &self,
        table: Self::Table,
        key: Self::Key<'a>,
    ) -> Result<Option<Self::ExtValue>, Self::Error> {
        let write_txn = self.begin_write()?;
        let val = {
            let mut write_table = write_txn.open_table(table)?;
            let val = write_table.remove(key)?;
            val.map(|x| Into::<Self::ExtValue>::into(x.value()))
        };
        write_txn.commit()?;

        Ok(val)
    }

    fn remove_iter<I: IntoIterator<Item = Self::Key<'a>>>(
        &self,
        table: Self::Table,
        keys: I,
    ) -> Result<Vec<Option<Self::ExtValue>>, Self::Error> {
        let mut values: Vec<Option<Self::ExtValue>> = Vec::new();

        let write_txn = self.begin_write()?;
        {
            let mut write_table = write_txn.open_table(table)?;
            for key in keys {
                values.push(
                    write_table
                        .remove(key)?
                        .map(|x| Into::<Self::ExtValue>::into(x.value())),
                );
            }
        }
        write_txn.commit()?;

        Ok(values)
    }

    fn modify<F>(&self, table: Self::Table, key: Self::Key<'a>, func: F) -> Result<(), Self::Error>
    where
        F: FnOnce(Option<Self::ExtValue>) -> Option<Self::ExtValue>,
    {
        let write_txn = self.begin_write()?;
        {
            let mut write_table = write_txn.open_table(table)?;
            let value: Option<Package> = write_table.remove(key)?.map(|x| x.value().into());
            if let Some(value) = func(value) {
                write_table.insert(key, Into::<Self::Value>::into(value))?;
            }
        };
        write_txn.commit()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use redb::{Database, ReadableTable, TableDefinition};

    use super::*;
    // use crate::init_logging;
    use crate::pkg::{Installed, Local, Package, PackageInfo, Source};

    #[test]
    fn test_redb() {
        // init_logging();

        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("test.db");

        let table: TableDefinition<&str, DbPackage> = TableDefinition::new("test");
        let db = Database::create(path).unwrap();
        let topgrade = Package {
                        info: PackageInfo {
                            name: "topgrade".to_owned(),
                            version: Version::from_str("12.0.2").unwrap(),
                            license: "GPL3.0".to_owned(),
                            repository: None, //Some("https://github.com/topgrade-rs/topgrade".to_owned()),
                            authors: Some(vec!["topgrade-rs".to_owned()]),
                            description: Some("Upgrade all the things".to_owned()),
                            dependencies: None,
                            build_dependencies: None,
                            provides: None,
                        },
                        source: Source {
                            url: "https://github.com/topgrade-rs/topgrade/releases/download/v12.0.2/topgrade-v12.0.2-x86_64-apple-darwin.tar.gz".to_owned(),
                            checksum: Some("45dfddf13e8f5a5eb4a95dde6743f42f216ed6d3751d7430dae5f9e0dc54e67a400e6572789fb9984ff1c80bdee42a92112a76d5399436e857e723b653b366f1".to_owned()),
                            build: None,
                            install: "mv ${source}/topgrade ${binary}".to_owned(),
                        },
                        local: Local { installed: Installed::False, added: true}
                    };

        let write_txn = db.begin_write().unwrap();
        {
            let mut write_table = write_txn.open_table(table).unwrap();
            write_table
                .insert("topgrade", Into::<DbPackage>::into(topgrade.clone()))
                .unwrap();
        }
        write_txn.commit().unwrap();

        let read_txn = db.begin_read().unwrap();
        let read_table = read_txn.open_table(table).unwrap();
        for pkg in read_table.iter().unwrap() {
            let pkg = pkg.unwrap();
            assert_eq!(dbg!(pkg.0.value()), "topgrade");
            assert_eq!(dbg!(pkg.1.value()), topgrade.clone().into());
        }
        assert_eq!(
            read_table.get("topgrade").unwrap().unwrap().value(),
            topgrade.into()
        );
    }
}
