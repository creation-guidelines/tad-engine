//! Starting a bare database: what every tad database stands on, and nothing of a domain - the
//! declaration with no table yet, the folder of the tables, tad pinned to the release that started it
//! (bin/install-tad.sh, its checksum checked) and the check CI runs. It took the place of
//! text-as-data-template's scaffold (decision 2026-11-17-tad-init).
use anyhow::{bail, Context, Result};
use std::path::Path;

/// What a bare database is made of: where each file goes and what it holds.
pub fn files(version: &str) -> Vec<(&'static str, String, bool)> {
    vec![
        (
            "tables.toml",
            include_str!("init/tables.toml").to_string(),
            false,
        ),
        ("data/.gitkeep", String::new(), false),
        (
            "bin/install-tad.sh",
            include_str!("init/install-tad.sh").replace("@VERSION@", version),
            true,
        ),
        (
            ".github/workflows/tad.yml",
            include_str!("init/tad.yml").to_string(),
            false,
        ),
    ]
}

/// Starts a bare database in `dir`, refusing - and writing nothing - where any of its files is there
/// already. Returns the paths written, the pages rendered from the empty declaration among them, so that
/// the check CI runs holds from the first commit.
pub fn init(dir: &Path, version: &str) -> Result<Vec<String>> {
    let files = files(version);
    let there: Vec<&str> = files
        .iter()
        .map(|(p, _, _)| *p)
        .filter(|p| dir.join(p).exists())
        .collect();
    if !there.is_empty() {
        bail!(
            "{} holds a database already ({} there): nothing was written",
            dir.display(),
            there.join(", ")
        );
    }
    let mut written = Vec::new();
    for (path, body, executable) in &files {
        let to = dir.join(path);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("making {}", parent.display()))?;
        }
        std::fs::write(&to, body).with_context(|| format!("writing {}", to.display()))?;
        #[cfg(unix)]
        if *executable {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o755))?;
        }
        written.push(path.to_string());
    }
    let declaration: crate::Declaration = toml::from_str(include_str!("init/tables.toml"))?;
    crate::render::render(
        &dir.join("data"),
        &declaration,
        &dir.join("docs"),
        &["md".to_string()],
    )?;
    written.push("docs/".to_string());
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("tad-init-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_bare_database_checks_out_and_pins_the_release_that_made_it() {
        let d = scratch("bare");
        let written = init(&d, "9.8.7").unwrap();
        assert!(written.contains(&"tables.toml".to_string()));
        let declaration: crate::Declaration =
            toml::from_str(&std::fs::read_to_string(d.join("tables.toml")).unwrap()).unwrap();
        assert!(declaration.tables.is_empty());
        let (rows, problems) = crate::check(&d.join("data"), &declaration).unwrap();
        assert_eq!((rows, problems.len()), (0, 0));
        let pin = std::fs::read_to_string(d.join("bin/install-tad.sh")).unwrap();
        assert!(pin.contains("TAD_VERSION:-9.8.7}"));
        assert!(!pin.contains("@VERSION@"));
        let ci = std::fs::read_to_string(d.join(".github/workflows/tad.yml")).unwrap();
        assert!(ci.contains("./bin/install-tad.sh") && ci.contains("tad check"));
        assert!(d.join("docs").is_dir());
        // what was rendered is what render writes again: the check CI runs holds from the first commit
        let before = std::fs::read_dir(d.join("docs")).unwrap().count();
        crate::render::render(
            &d.join("data"),
            &declaration,
            &d.join("docs"),
            &["md".into()],
        )
        .unwrap();
        assert_eq!(before, std::fs::read_dir(d.join("docs")).unwrap().count());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_place_that_holds_a_database_is_left_as_it_is() {
        let d = scratch("there");
        std::fs::write(d.join("tables.toml"), "[tables]\n# mine\n").unwrap();
        let e = init(&d, "1.0.0").unwrap_err().to_string();
        assert!(e.contains("tables.toml there"), "{e}");
        assert_eq!(
            std::fs::read_to_string(d.join("tables.toml")).unwrap(),
            "[tables]\n# mine\n"
        );
        assert!(!d.join("data").exists() && !d.join("bin").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
