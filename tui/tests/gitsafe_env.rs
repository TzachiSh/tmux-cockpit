//! `cockpit-tui diffstat` with a git variable inherited. The diff count builds
//! `:(literal)` and `:(exclude,literal)` pathspecs, and the GIT_*_PATHSPECS
//! variables change what those match (#673); others move the object store or
//! the attributes git reads (#683). Each test sets one on the child alone,
//! never on this process, and wants the count plain git gives with none set.
use std::path::PathBuf;
use std::process::Command;

/// Every variable that changes how git reads a pathspec.
const PATHSPEC_VARS: [&str; 4] = [
    "GIT_LITERAL_PATHSPECS",
    "GIT_GLOB_PATHSPECS",
    "GIT_NOGLOB_PATHSPECS",
    "GIT_ICASE_PATHSPECS",
];

/// A throwaway repo under the temp dir, removed on drop.
struct Repo(PathBuf);

impl Repo {
    fn new(tag: &str) -> Repo {
        let name = format!("cockpit-gitsafe-env-{tag}-{}", std::process::id());
        let d = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let r = Repo(d);
        r.git(&["init", "-q", "-b", "main"]);
        r
    }

    /// Plain git in the repo, with no pathspec variable even if this process has one.
    fn git(&self, args: &[&str]) -> Vec<u8> {
        let mut c = Command::new("git");
        for v in PATHSPEC_VARS {
            c.env_remove(v);
        }
        let o = c
            .arg("-C")
            .arg(&self.0)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        o.stdout
    }

    fn write(&self, name: &str, body: &str) {
        std::fs::write(self.0.join(name), body).unwrap();
    }

    fn commit(&self) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", "c"]);
    }

    /// Plain git's count: added and deleted lines off `diff HEAD`, files off `ls-files -o`.
    fn reference(&self) -> (u64, u64, u64) {
        let numstat = String::from_utf8(self.git(&["diff", "HEAD", "--numstat"])).unwrap();
        let field = |l: &str, k: usize| l.split('\t').nth(k).unwrap().parse::<u64>().unwrap();
        let col = |k: usize| numstat.lines().map(|l| field(l, k)).sum();
        let others = self.git(&["ls-files", "-o", "--exclude-standard", "-z"]);
        (col(0), col(1), others.split(|b| *b == 0).filter(|p| !p.is_empty()).count() as u64)
    }

    /// diffstat's exit status and its first three fields, with each of `env` set to 1.
    fn diffstat(&self, env: &[&str]) -> (i32, Option<(u64, u64, u64)>) {
        let pairs: Vec<(&str, &str)> = env.iter().map(|v| (*v, "1")).collect();
        self.diffstat_with(&pairs)
    }

    /// [`Repo::diffstat`] with `env` set on the child only, and the runner's own
    /// git config kept out, as [`Repo::git`] keeps it out of the reference.
    fn diffstat_with(&self, env: &[(&str, &str)]) -> (i32, Option<(u64, u64, u64)>) {
        let mut c = Command::new(env!("CARGO_BIN_EXE_cockpit-tui"));
        for v in PATHSPEC_VARS {
            c.env_remove(v);
        }
        c.env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1");
        c.envs(env.iter().copied());
        let o = c.arg("diffstat").arg(&self.0).output().unwrap();
        let out = String::from_utf8_lossy(&o.stdout).into_owned();
        let n: Vec<u64> = out.split('\t').take(3).filter_map(|f| f.parse().ok()).collect();
        (o.status.code().unwrap_or(-1), (n.len() == 3).then(|| (n[0], n[1], n[2])))
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `A` (one line) and `a` (two) committed, then a plain `mv A B`: the count
/// takes A's lines through `base_lines`, a `:(literal)A` pathspec.
fn moved_capital_a(tag: &str) -> Repo {
    let r = Repo::new(tag);
    r.write("A", "1\n");
    r.write("a", "1\n2\n");
    r.commit();
    std::fs::rename(r.0.join("A"), r.0.join("B")).unwrap();
    r
}

/// `core` and `f` committed and each given a line, and an untracked sparse
/// `Core` past the hashing cap: the diff runs with `:(exclude,literal)Core`.
fn big_capital_core(tag: &str) -> Repo {
    let r = Repo::new(tag);
    r.write("core", "1\n");
    r.write("f", "x\n");
    r.commit();
    r.write("core", "1\n2\n");
    r.write("f", "x\ny\n");
    std::fs::File::create(r.0.join("Core")).unwrap().set_len(33 * 1024 * 1024).unwrap();
    r
}

#[test]
fn the_fixtures_count_what_git_does_with_nothing_inherited() {
    let r = moved_capital_a("ctl-mv");
    assert_eq!(r.reference(), (0, 1, 1), "A's one line, B new");
    assert_eq!(r.diffstat(&[]), (0, Some(r.reference())));
    let r = big_capital_core("ctl-big");
    assert_eq!(r.reference(), (2, 0, 1), "core and f a line each, Core new");
    assert_eq!(r.diffstat(&[]), (0, Some(r.reference())));
}

/// LITERAL reads `:(literal)A` as a path named so, which matches nothing.
#[test]
fn literal_pathspecs_inherited_still_count_a_moved_files_lines() {
    let r = moved_capital_a("lit-mv");
    assert_eq!(r.diffstat(&["GIT_LITERAL_PATHSPECS"]), (0, Some(r.reference())));
}

/// LITERAL turns `:(exclude,literal)Core` into the only path the diff may show.
#[test]
fn literal_pathspecs_inherited_still_count_beside_a_big_file() {
    let r = big_capital_core("lit-big");
    assert_eq!(r.diffstat(&["GIT_LITERAL_PATHSPECS"]), (0, Some(r.reference())));
}

/// ICASE makes `:(literal)A` match `a` too, so a's lines count as A's.
#[test]
fn icase_pathspecs_inherited_count_a_moved_file_without_its_lowercase_twin() {
    let r = moved_capital_a("icase-mv");
    assert_eq!(r.diffstat(&["GIT_ICASE_PATHSPECS"]), (0, Some(r.reference())));
}

/// ICASE makes `:(exclude,literal)Core` drop `core`'s change too.
#[test]
fn icase_pathspecs_inherited_keep_a_file_whose_big_twin_is_excluded() {
    let r = big_capital_core("icase-big");
    assert_eq!(r.diffstat(&["GIT_ICASE_PATHSPECS"]), (0, Some(r.reference())));
}

/// `:(literal)` overrides GLOB, so alone it moves nothing: this pins that.
#[test]
fn glob_pathspecs_inherited_count_what_git_does() {
    let r = moved_capital_a("glob-mv");
    assert_eq!(r.diffstat(&["GIT_GLOB_PATHSPECS"]), (0, Some(r.reference())));
}

/// NOGLOB only adds `literal` where `:(literal)` already is: this pins that.
#[test]
fn noglob_pathspecs_inherited_count_what_git_does() {
    let r = moved_capital_a("noglob-mv");
    assert_eq!(r.diffstat(&["GIT_NOGLOB_PATHSPECS"]), (0, Some(r.reference())));
}

/// git refuses GLOB and NOGLOB together on any pathspec, and the row read `?`.
#[test]
fn glob_and_noglob_inherited_together_still_count() {
    let r = moved_capital_a("both-mv");
    let both = ["GIT_GLOB_PATHSPECS", "GIT_NOGLOB_PATHSPECS"];
    assert_eq!(r.diffstat(&both), (0, Some(r.reference())));
}

/// `f` committed with one line and given a second: plain git counts `+1 -0`.
fn one_line_added(tag: &str) -> Repo {
    let r = Repo::new(tag);
    r.write("f", "1\n");
    r.commit();
    r.write("f", "1\n2\n");
    r
}

/// GIT_OBJECT_DIRECTORY naming no dir: git finds no repo and the row read `?`.
#[test]
fn an_object_directory_inherited_that_is_missing_still_counts() {
    let r = one_line_added("objdir-missing");
    let gone = r.0.join("no-such-objects");
    let env = [("GIT_OBJECT_DIRECTORY", gone.to_str().unwrap())];
    assert_eq!(r.diffstat_with(&env), (0, Some(r.reference())));
}

/// GIT_OBJECT_DIRECTORY naming an empty dir: HEAD's commit is not in it.
#[test]
fn an_object_directory_inherited_that_is_empty_still_counts() {
    let r = one_line_added("objdir-empty");
    let empty = r.0.join(".git/empty-objects");
    std::fs::create_dir(&empty).unwrap();
    let env = [("GIT_OBJECT_DIRECTORY", empty.to_str().unwrap())];
    assert_eq!(r.diffstat_with(&env), (0, Some(r.reference())));
}

/// GIT_ATTR_SOURCE naming no rev: every diff is fatal and the row read `?`.
#[test]
fn an_attr_source_inherited_that_is_no_rev_still_counts() {
    let r = one_line_added("attr-norev");
    assert_eq!(r.diffstat_with(&[("GIT_ATTR_SOURCE", "no-such-rev")]), (0, Some(r.reference())));
}

/// GIT_ATTR_SOURCE naming a tree whose `.gitattributes` says `f -diff`: f's
/// lines would read as binary, `-` for each count, and add up to nothing.
#[test]
fn an_attr_source_inherited_from_another_tree_still_counts() {
    let r = one_line_added("attr-tree");
    r.write(".gitattributes", "f -diff\n");
    r.git(&["add", ".gitattributes"]);
    let tree = String::from_utf8(r.git(&["write-tree"])).unwrap();
    r.git(&["rm", "-q", "--cached", ".gitattributes"]);
    std::fs::remove_file(r.0.join(".gitattributes")).unwrap();
    assert_eq!(r.reference(), (1, 0, 0), "f's one new line, nothing untracked");
    assert_eq!(r.diffstat_with(&[("GIT_ATTR_SOURCE", tree.trim())]), (0, Some(r.reference())));
}

/// An executable at `.git/<name>` that touches `.git/<name>.ran` when run:
/// in the git dir, so neither the count nor plain git's reference sees it.
fn marker_script(r: &Repo, name: &str, tail: &str) -> (String, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let p = r.0.join(".git").join(name);
    std::fs::write(&p, format!("#!/bin/sh\ntouch \"$0.ran\"\n{tail}")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    let ran = r.0.join(".git").join(format!("{name}.ran"));
    (p.display().to_string(), ran)
}

/// An fsmonitor hook injected by GIT_CONFIG_COUNT: SAFE's own
/// `core.fsmonitor=false` comes later on git's list and wins.
#[test]
fn an_fsmonitor_from_config_count_inherited_never_runs() {
    let r = one_line_added("cfgcount-fsm");
    let (hook, ran) = marker_script(&r, "fsm", "");
    let env = [
        ("GIT_CONFIG_COUNT", "1"),
        ("GIT_CONFIG_KEY_0", "core.fsmonitor"),
        ("GIT_CONFIG_VALUE_0", hook.as_str()),
    ];
    assert_eq!(r.diffstat_with(&env), (0, Some(r.reference())));
    assert!(!ran.exists(), "the injected fsmonitor hook ran");
}

/// A clean filter injected by GIT_CONFIG_PARAMETERS for a tracked file the
/// repo's attributes route through it: the no_filters scan reads env config
/// too, and blanks it.
#[test]
fn a_filter_from_config_parameters_inherited_never_runs() {
    let r = Repo::new("cfgparams-filter");
    r.write(".gitattributes", "f filter=envf\n");
    r.write("f", "1\n");
    r.commit();
    r.write("f", "1\n2\n");
    let (clean, ran) = marker_script(&r, "clean", "cat\n");
    let params = format!("'filter.envf.clean'='{clean}'");
    assert_eq!(r.diffstat_with(&[("GIT_CONFIG_PARAMETERS", &params)]), (0, Some(r.reference())));
    assert!(!ran.exists(), "the injected clean filter ran");
}

/// [`one_line_added`] with `f`'s committed blob replaced under `<base><blob>`
/// by a three-line one: through it, plain git's `+1 -0` reads `+2 -3` (#730).
fn blob_replaced_under(tag: &str, base: &str) -> Repo {
    let r = one_line_added(tag);
    let old = String::from_utf8(r.git(&["rev-parse", "HEAD:f"])).unwrap();
    std::fs::write(r.0.join(".git/swap"), "x\ny\nz\n").unwrap();
    let new = String::from_utf8(r.git(&["hash-object", "-w", ".git/swap"])).unwrap();
    r.git(&["update-ref", &format!("{base}{}", old.trim()), new.trim()]);
    r
}

/// GIT_REPLACE_REF_BASE naming a base that holds a replacement: f's lines
/// would count from the swapped blob, not the one HEAD stores.
#[test]
fn a_replace_ref_base_inherited_never_swaps_a_blob() {
    let r = blob_replaced_under("replace-alt", "refs/alt/");
    assert_eq!(r.reference(), (1, 0, 0), "plain git reads the stored blob");
    let env = [("GIT_REPLACE_REF_BASE", "refs/alt/")];
    assert_eq!(r.diffstat_with(&env), (0, Some(r.reference())));
}

/// GIT_REPLACE_REF_BASE naming an empty base: the repo's own refs/replace/
/// would stop applying, and the count part from what plain git shows.
#[test]
fn a_replace_ref_base_inherited_never_hides_the_repos_own_replacement() {
    let r = blob_replaced_under("replace-own", "refs/replace/");
    assert_eq!(r.reference(), (2, 3, 0), "plain git reads the repo's replacement");
    let env = [("GIT_REPLACE_REF_BASE", "refs/none/")];
    assert_eq!(r.diffstat_with(&env), (0, Some(r.reference())));
}

/// GIT_NO_REPLACE_OBJECTS inherited: the repo's own refs/replace/ would stop
/// applying, so the count would leave git's default view of the repo.
#[test]
fn no_replace_objects_inherited_never_hides_the_repos_own_replacement() {
    let r = blob_replaced_under("noreplace", "refs/replace/");
    assert_eq!(r.reference(), (2, 3, 0), "plain git reads the repo's replacement");
    let env = [("GIT_NO_REPLACE_OBJECTS", "1")];
    assert_eq!(r.diffstat_with(&env), (0, Some(r.reference())));
}

/// scripts/lib/git-env.sh's copy of the strip list, which tmux-review,
/// tmux-branch and the review diff unset, holds exactly what gitsafe strips (#730).
#[test]
fn git_env_sh_strips_the_same_variables_as_gitsafe() {
    use cockpit_tui::data::gitsafe::{PATHSPEC, REDIRECT};
    let lib = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/lib/git-env.sh");
    let o = Command::new("bash")
        .args(["-c", r#". "$1" && printf '%s\n' "${GIT_ENV_STRIP[@]}""#, "_", lib])
        .output()
        .unwrap();
    assert!(o.status.success(), "sourcing git-env.sh: {}", String::from_utf8_lossy(&o.stderr));
    let out = String::from_utf8(o.stdout).unwrap();
    let mut got: Vec<&str> = out.lines().collect();
    let mut want: Vec<&str> = REDIRECT.iter().chain(&PATHSPEC).copied().collect();
    got.sort();
    want.sort();
    assert_eq!(got, want);
}
