use convert_case::{Boundary, Case, Casing, Pattern};
use std::{
    fmt::{self, Display, Formatter},
    fs,
    io::Write,
    iter::Iterator,
    path::{Path, PathBuf},
    process::Command,
};

pub fn generate_bindings(source_path: &Path) -> crate::Result<PathBuf> {
    let bindings = crate::read_bindings(source_path)?;
    let parsed = syn::parse_file(&bindings)?;
    let parse_tree = ParseTree::from(&parsed);

    let mut out_file = crate::dirs::get_out_dir();
    out_file.push("bindings.rs");
    let mut bindings = fs::File::create(&out_file)?;
    write!(bindings, "{parse_tree}")?;
    format_bindings(&out_file)?;

    Ok(out_file)
}

enum Resource {
    Control(String),
    Packed(String),
    String(String),
}

impl Resource {
    fn cef_fn(&self) -> &str {
        match self {
            Resource::Control(_) => "cef_id_for_command_id_name",
            Resource::Packed(_) => "cef_id_for_pack_resource_name",
            Resource::String(_) => "cef_id_for_pack_string_name",
        }
    }

    fn sys_name(&self) -> &str {
        match self {
            Resource::Control(name) | Resource::Packed(name) | Resource::String(name) => {
                name.as_str()
            }
        }
    }

    fn name(&self) -> String {
        self.sys_name()
            .from_case(Case::Custom {
                boundaries: &[Boundary::LowerUpper, Boundary::Acronym],
                pattern: Pattern::Capital,
                delimiter: "",
            })
            .to_case(Case::Constant)
    }
}

#[derive(Default)]
struct ParseTree {
    resources: Vec<Resource>,
}

impl ParseTree {
    pub fn write_prelude(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(f, "use std::ffi::CStr;")
    }

    pub fn write_resources(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for resource in &self.resources {
            let cef_fn = resource.cef_fn();
            let sys_name = resource.sys_name();
            let name = resource.name();
            writeln!(
                f,
                r#"
/// `"{sys_name}"`: Resource ID for use with [`cef_dll_sys::{cef_fn}`].
pub const {name}: &CStr = c"{sys_name}";"#
            )?;
        }
        Ok(())
    }

    pub fn write_tests(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            r#"
#[cfg(test)]
mod tests {{
    use crate::*;

    #[test]
    fn test_resource_mapping() {{
        test_init_cef();"#
        )?;

        if let Some(first_control) = self
            .resources
            .iter()
            .find(|resource| matches!(resource, Resource::Control(_)))
        {
            Self::write_assert(f, first_control)?;
        }

        if let Some(first_packed) = self
            .resources
            .iter()
            .find(|resource| matches!(resource, Resource::Packed(_)))
        {
            Self::write_assert(f, first_packed)?;
        }

        if let Some(first_string) = self
            .resources
            .iter()
            .find(|resource| matches!(resource, Resource::String(_)))
        {
            Self::write_assert(f, first_string)?;
        }

        writeln!(
            f,
            r#"    }}
}}"#
        )
    }

    fn write_assert(f: &mut Formatter<'_>, resource: &Resource) -> fmt::Result {
        let cef_fn = resource.cef_fn();
        let sys_name = resource.sys_name();
        let name = resource.name();

        writeln!(
            f,
            r#"
    let resource_id = unsafe {{ sys::{cef_fn}(resources::{name}.as_ptr()) }};
    assert!(resource_id == sys::{sys_name} || resource_id == -1);"#
        )
    }
}

impl Display for ParseTree {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.write_prelude(f)?;
        self.write_resources(f)?;
        self.write_tests(f)
    }
}

impl From<&syn::File> for ParseTree {
    fn from(value: &syn::File) -> Self {
        Self {
            resources: value
                .items
                .iter()
                .filter_map(|item| match item {
                    syn::Item::Const(syn::ItemConst {
                        vis: syn::Visibility::Public(_),
                        ident,
                        ..
                    }) => {
                        let name = ident.to_string();
                        if name.starts_with("IDC_") {
                            Some(Resource::Control(name))
                        } else if name.starts_with("IDR_") {
                            Some(Resource::Packed(name))
                        } else if name.starts_with("IDS_") {
                            Some(Resource::String(name))
                        } else {
                            None
                        }
                    }
                    _ => None,
                })
                .collect(),
        }
    }
}

fn format_bindings(source_path: &Path) -> crate::Result<()> {
    let mut cmd = Command::new(env!("CARGO"));
    cmd.args(["fmt", "--", &source_path.display().to_string()]);
    cmd.output()?;
    Ok(())
}
