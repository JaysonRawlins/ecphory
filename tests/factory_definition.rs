//! Pins the swamp software factory (`models/@swamp/software-factory/ecphory-factory.yaml`).
//!
//! `swamp model method run ecphory-factory validate` checks the schema and that
//! every stage is reachable. It accepts a definition that skips the plan
//! refute, lets live-verify run against the live daemon, or opens a PR before
//! the OpenSpec change is archived. Nothing else checks the stage graph or the
//! prompt rules below, so they are pinned here.
//!
//! The shape comes from exman's factory, adapted: a GitHub issue is the work
//! item, pre-pr-verify runs cargo instead of pnpm/cdk, and live-verify runs a
//! scratch `ecphory serve` rather than deploying a stack. There is no
//! attestation manifest. The run ends when the PR merges.
//!
//! Red-proofs, 2026-10-19: all 20 failed before the definition existed. Then
//! one edit at a time to the definition, each failing only the named test(s):
//! plan-verify passing straight to implementing (the graph and refute pins);
//! dropping implementation-approval; maxCycles 2 -> 5; "never 3491" softened
//! to "not 3491"; final-pre-pr-verify on the change-request sha instead of the
//! archive sha; medium dropped from the review's rework CEL; the no-footer rule
//! removed; "exman" added to a description; and a workflow step naming a model
//! with no file.

use serde_json::{Value, json};
use std::path::PathBuf;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load(rel: &str) -> Value {
    let path = repo().join(rel);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_yaml_ng::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn factory() -> Value {
    load("models/@swamp/software-factory/ecphory-factory.yaml")
}

fn stages() -> Vec<Value> {
    factory()["globalArguments"]["stages"]
        .as_array()
        .expect("stages")
        .clone()
}

fn stage(id: &str) -> Value {
    stages()
        .into_iter()
        .find(|s| s["id"] == id)
        .unwrap_or_else(|| panic!("no stage {id}"))
}

/// Transitions out of `from` that lead to `to`.
fn transitions(from: &str, to: &str) -> Vec<Value> {
    stage(from)["transitions"]
        .as_array()
        .map(|ts| ts.iter().filter(|t| t["to"] == to).cloned().collect())
        .unwrap_or_default()
}

fn gates(from: &str, to: &str) -> Vec<Value> {
    transitions(from, to)
        .iter()
        .map(|t| t["gates"].clone())
        .collect()
}

/// The stage's system prompt with whitespace collapsed, as the YAML folds it.
fn prompt(id: &str) -> String {
    stage(id)["work"]["systemPrompt"]
        .as_str()
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

const PLAN_SLUG: &str = r#"${{ data.latest(self.name, "artifact-plan").payload.changeSlug }}"#;
const HEAD_SHA: &str =
    r#"${{ data.latest(self.name, "evidence-change-request").payload.headSha }}"#;
const BRANCH: &str = r#"${{ data.latest(self.name, "evidence-change-request").payload.branch }}"#;

fn blocking_open(artifact: &str) -> String {
    format!(
        "size(artifacts.{artifact}.findings) == 0 || artifacts.{artifact}.findings.exists(f, \
         (!has(f.resolved) || !f.resolved) && (f.severity == \"critical\" || f.severity == \"high\" \
         || f.severity == \"medium\"))"
    )
}

#[test]
fn the_stage_graph_is_exactly_this() {
    let mut edges: Vec<String> = stages()
        .iter()
        .flat_map(|s| {
            let id = s["id"].as_str().unwrap().to_string();
            s["transitions"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(move |t| {
                    format!(
                        "{id} --{}--> {}",
                        t["name"].as_str().unwrap(),
                        t["to"].as_str().unwrap()
                    )
                })
        })
        .collect();
    edges.sort();
    let mut expected = vec![
        "intake --submit--> planning",
        "planning --submit--> openspec-plan-verify",
        "openspec-plan-verify --pass--> plan-refute",
        "openspec-plan-verify --fail--> planning",
        "openspec-plan-verify --rework--> planning",
        "plan-refute --pass--> implementing",
        "plan-refute --rework--> planning",
        "implementing --submit--> openspec-tasks-verify",
        "openspec-tasks-verify --pass--> pre-pr-verify",
        "openspec-tasks-verify --fail--> implementing",
        "pre-pr-verify --pass--> adversarial-review",
        "pre-pr-verify --fail--> implementing",
        "adversarial-review --pass--> live-verify",
        "adversarial-review --rework--> implementing",
        "live-verify --pass--> reconcile",
        "live-verify --blocked--> reconcile",
        "live-verify --fail--> implementing",
        "reconcile --pass--> archive",
        "reconcile --rework--> implementing",
        "archive --submit--> openspec-archive-verify",
        "openspec-archive-verify --pass--> final-pre-pr-verify",
        "openspec-archive-verify --fail--> archive",
        "final-pre-pr-verify --pass--> open-pr",
        "final-pre-pr-verify --fail--> implementing",
        "open-pr --submit--> in-review",
        "open-pr --rework--> implementing",
        "in-review --rework--> implementing",
        "in-review --merged--> done",
    ];
    expected.sort();
    assert_eq!(edges, expected);

    let first = &stages()[0];
    assert_eq!(
        (first["id"].as_str(), first["initial"].as_bool()),
        (Some("intake"), Some(true))
    );
    for id in ["done", "aborted"] {
        assert_eq!(stage(id)["terminal"], json!(true), "{id} must be terminal");
    }
    assert_eq!(
        factory()["globalArguments"]["globalTransitions"],
        json!([{"name": "abort", "to": "aborted", "gates": [{"type": "human-approval", "config": {"id": "abort-confirmation"}}]}])
    );
}

mod intake {
    use super::*;

    #[test]
    fn the_work_item_is_a_github_issue_read_with_gh() {
        let text = prompt("intake");
        assert!(
            text.contains("gh issue view ${{ self.workItem }} --repo JaysonRawlins/ecphory"),
            "{text}"
        );
        assert_eq!(stage("intake")["artifacts"][0]["name"], "issue-context");
        assert_eq!(
            gates("intake", "planning"),
            vec![json!([{"type": "artifact-exists", "config": {"artifact": "issue-context"}}])]
        );
    }

    /// The swamp identity is the session's: a personal session starts under
    /// `doppler run -p swamp -c personal`, and the employer login is out of
    /// swamp's default config dir, so any other session is unauthenticated. A
    /// session that still answers as someone else is stopped here, before the
    /// issue is read. RED-PROOF, 2026-10-19: absent on a6f8195; dropping the
    /// username printed `lacks "jaysonjrawlins"`; whoami moved after `gh issue
    /// view` printed "does not check the swamp identity before reading".
    #[test]
    fn intake_checks_the_swamp_identity_before_reading_the_issue() {
        let text = prompt("intake");
        let whoami = text.find("swamp auth whoami --json");
        let issue = text.find("gh issue view");
        assert!(
            matches!((whoami, issue), (Some(w), Some(i)) if w < i),
            "intake does not check the swamp identity before reading the issue:\n{text}"
        );
        for want in ["jaysonjrawlins", "stop", "doppler run -p swamp -c personal"] {
            assert!(text.contains(want), "intake identity check lacks {want:?}");
        }
    }
}

mod plan {
    use super::*;

    #[test]
    fn planning_writes_an_openspec_change_with_live_checks_outside_the_tasks() {
        let text = prompt("planning");
        assert!(text.contains("openspec/changes/<changeSlug>/"));
        assert!(text.contains("\"## Live verification\""));
        assert!(text.contains("without checkboxes"));
    }

    #[test]
    fn a_verified_change_goes_to_the_refute_with_no_human_gate_yet() {
        assert_eq!(
            gates("openspec-plan-verify", "plan-refute"),
            vec![
                json!([{"type": "workflow-succeeded", "config": {"workflow": "openspec-plan-verify"}}])
            ]
        );
        assert!(transitions("openspec-plan-verify", "implementing").is_empty());
        let verify = stage("openspec-plan-verify");
        assert_eq!(
            verify["work"]["workflow"],
            json!({"name": "openspec-plan-verify", "inputs": {"changeSlug": PLAN_SLUG}})
        );
    }

    #[test]
    fn a_failed_or_rejected_verification_returns_to_planning() {
        let back = transitions("openspec-plan-verify", "planning");
        let fail = back.iter().find(|t| t["name"] == "fail").expect("fail");
        assert_eq!(
            fail["gates"],
            json!([{"type": "evidence-recorded", "config": {"name": "plan-verify-run", "requireField": {"status": "failed"}}}])
        );
        let rework = back.iter().find(|t| t["name"] == "rework").expect("rework");
        assert_eq!(rework["gates"], json!([]));
    }

    // Each fresh reviewer finds a new layer, so refuting until one finds nothing
    // measures when the sampling stopped. A third refute needs a cycle-override.
    #[test]
    fn one_reviewer_refutes_the_plan_at_most_twice() {
        let refute = stage("plan-refute");
        assert_eq!(refute["maxCycles"], json!(2));
        assert_eq!(refute["work"]["mode"], "dispatch");
        assert_eq!(refute["work"]["skills"], json!(["plan-refute"]));
        assert_eq!(
            refute["artifacts"],
            json!([{"name": "plan-refute", "kind": "findings", "reviews": "plan"}])
        );
        let text = prompt("plan-refute");
        assert!(text.contains(&format!("openspec/changes/{PLAN_SLUG}/")));
        assert!(text.contains("findings from an earlier cycle"));
        assert!(text.contains("keep each earlier finding's id"));
        assert!(text.contains("at most 8 findings"));
    }

    #[test]
    fn the_approver_decides_at_the_refute_on_this_cycles_findings() {
        assert_eq!(
            gates("plan-refute", "implementing"),
            vec![json!([
                {"type": "artifact-fresh", "config": {"artifact": "plan-refute", "recordedThisCycle": true}},
                {"type": "human-approval", "config": {"id": "implementation-approval"}},
            ])]
        );
        assert_eq!(gates("plan-refute", "planning"), vec![json!([])]);
    }
}

mod implement {
    use super::*;

    #[test]
    fn implementing_restores_an_archived_change_and_commits_conventionally() {
        let text = prompt("implementing");
        assert!(text.contains("openspec/changes/archive/"));
        assert!(text.contains("revert the archive commit"));
        // release-plz cuts releases from conventional commit messages.
        assert!(text.contains("conventional commit"));
    }

    #[test]
    fn tasks_then_pre_pr_verification_gate_the_implementation() {
        assert_eq!(
            gates("openspec-tasks-verify", "pre-pr-verify"),
            vec![
                json!([{"type": "workflow-succeeded", "config": {"workflow": "openspec-tasks-verify"}}])
            ]
        );
        let verify = stage("pre-pr-verify");
        assert_eq!(
            verify["work"]["workflow"],
            json!({"name": "pre-pr-verify", "inputs": {"commit": HEAD_SHA, "branch": BRANCH}})
        );
        assert_eq!(verify["work"]["resultEvidence"], "verify-run");
        assert_eq!(
            gates("pre-pr-verify", "implementing"),
            vec![
                json!([{"type": "evidence-recorded", "config": {"name": "verify-run", "requireField": {"status": "failed"}}}])
            ]
        );
    }
}

mod review {
    use super::*;

    #[test]
    fn two_lanes_review_the_change_and_blocking_findings_send_it_back() {
        let review = stage("adversarial-review");
        assert_eq!(
            review["work"]["skills"],
            json!(["architecture", "security-performance"])
        );
        assert_eq!(
            review["artifacts"],
            json!([{"name": "adversarial-review", "kind": "findings", "reviews": "change-summary"}])
        );
        let pass = &gates("adversarial-review", "live-verify")[0];
        assert_eq!(
            pass[0],
            json!({"type": "artifact-fresh", "config": {"artifact": "adversarial-review", "recordedThisCycle": true}})
        );
        assert_eq!(
            pass[1],
            json!({"type": "findings-clear", "config": {"artifact": "adversarial-review", "blocking": ["critical", "high", "medium"]}})
        );
        assert_eq!(
            pass[2]["config"]["expr"],
            "size(artifacts.adversarial_review.findings) >= 1"
        );
        let rework = &gates("adversarial-review", "implementing")[0];
        let expr = rework[0]["config"]["expr"]
            .as_str()
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(expr, blocking_open("adversarial_review"));
    }
}

mod live {
    use super::*;

    #[test]
    fn live_verify_resolves_the_changes_live_checks_and_binds_to_the_commit() {
        let text = prompt("live-verify");
        assert!(text.contains("\"## Live verification\""));
        let schema = &stage("live-verify")["evidence"][0];
        assert_eq!(schema["name"], "live-verification");
        assert_eq!(
            schema["schema"]["required"],
            json!(["changeSlug", "headSha", "criteria"])
        );
    }

    // The live daemon is Jayson's real memory store, serving every agent
    // session. An unmerged build must never reach it, its port or its mirror.
    #[test]
    fn live_verify_runs_a_scratch_instance_and_never_the_daemon() {
        let text = prompt("live-verify");
        for needle in [
            "ecphory serve",
            "--db",
            "--port",
            "never 3491",
            "never ~/.local/share/ecphory",
            "ECPHORY_EXPORT_DIR",
            "ECPHORY_URL",
            "under your own worktree",
        ] {
            assert!(text.contains(needle), "live-verify prompt lacks {needle:?}");
        }
        assert!(
            !text.contains("deploy-daemon"),
            "live-verify must not deploy to the daemon"
        );
    }

    #[test]
    fn live_verify_passes_or_is_waived_into_reconcile_and_never_straight_to_a_pr() {
        let into: Vec<Value> = transitions("live-verify", "reconcile");
        let blocked = into
            .iter()
            .find(|t| t["name"] == "blocked")
            .expect("blocked");
        assert_eq!(
            blocked["gates"][1],
            json!({"type": "human-approval", "config": {"id": "live-verification-waiver"}})
        );
        assert!(transitions("live-verify", "open-pr").is_empty());
    }
}

mod archive {
    use super::*;

    #[test]
    fn reconcile_reviews_every_touched_requirement_and_edits_nothing() {
        let reconcile = stage("reconcile");
        assert_eq!(reconcile["work"]["skills"], json!(["spec-reconcile"]));
        assert_eq!(
            reconcile["artifacts"],
            json!([{"name": "reconcile", "kind": "findings", "reviews": "change-summary"}])
        );
        let text = prompt("reconcile");
        assert!(text.contains("add, modify, remove or rename"));
        assert!(text.contains("every existing requirement under openspec/specs/"));
        assert!(text.contains("Do not edit any file"));
        let rework = &gates("reconcile", "implementing")[0];
        let expr = rework[0]["config"]["expr"]
            .as_str()
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(expr, blocking_open("reconcile"));
    }

    #[test]
    fn archive_runs_openspec_archive_and_a_workflow_checks_it() {
        assert!(prompt("archive").contains(&format!("openspec archive {PLAN_SLUG} --yes")));
        assert!(prompt("archive").contains("drops a scenario the current spec has"));
        assert!(prompt("archive").contains("replace the placeholder Purpose"));
        assert_eq!(
            stage("archive")["evidence"][0]["schema"]["required"],
            json!(["changeSlug", "archivedPath", "headSha"])
        );
        assert_eq!(
            gates("openspec-archive-verify", "final-pre-pr-verify"),
            vec![
                json!([{"type": "workflow-succeeded", "config": {"workflow": "openspec-archive-verify"}}])
            ]
        );
    }

    #[test]
    fn pre_pr_verification_runs_again_on_the_archive_commit() {
        let fin = stage("final-pre-pr-verify");
        assert_eq!(
            fin["work"]["workflow"],
            json!({"name": "pre-pr-verify", "inputs": {
                "commit": r#"${{ data.latest(self.name, "evidence-openspec-archive").payload.headSha }}"#,
                "branch": BRANCH,
            }})
        );
        assert_eq!(fin["work"]["resultEvidence"], "final-verify-run");
        let into: Vec<String> = stages()
            .iter()
            .filter(|s| {
                s["transitions"]
                    .as_array()
                    .is_some_and(|ts| ts.iter().any(|t| t["to"] == "open-pr"))
            })
            .map(|s| s["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(into, vec!["final-pre-pr-verify"]);
    }
}

mod pr {
    use super::*;

    #[test]
    fn open_pr_uses_gh_with_the_final_runs_evidence_and_no_ai_footer() {
        let text = prompt("open-pr");
        assert!(text.contains("gh pr create"));
        assert!(text.contains("Closes #"));
        assert!(text.contains("final-verify-run evidence"));
        assert!(text.contains("openspec-archive evidence"));
        assert!(text.contains("Co-Authored-By: Claude <noreply@anthropic.com>"));
        assert!(text.contains("no \"Generated with Claude Code\" footer"));
        assert_eq!(
            gates("open-pr", "implementing"),
            vec![json!([{"type": "human-approval", "config": {"id": "open-pr-rework-approval"}}])]
        );
    }

    #[test]
    fn the_run_ends_when_the_pr_merges_through_merge_pr() {
        assert!(prompt("in-review").contains("merge-pr"));
        assert_eq!(
            gates("in-review", "done"),
            vec![json!([{"type": "evidence-recorded", "config": {"name": "pr-merged"}}])]
        );
        assert_eq!(
            gates("in-review", "implementing"),
            vec![json!([{"type": "human-approval", "config": {"id": "rework-approval"}}])]
        );
    }
}

mod adaptation {
    use super::*;

    /// exman-specific machinery that must not leak into ecphory's factory.
    #[test]
    fn no_exman_machinery_remains() {
        let text = std::fs::read_to_string(
            repo().join("models/@swamp/software-factory/ecphory-factory.yaml"),
        )
        .unwrap();
        for needle in [
            "exman",
            "pnpm",
            "cdk",
            "attestation",
            "DSQL",
            "migration",
            "Datadog",
            "x-amzn",
            "vault.get",
        ] {
            assert!(
                !text.to_lowercase().contains(&needle.to_lowercase()),
                "factory still mentions {needle:?}"
            );
        }
    }

    /// The repo no longer routes swamp through a wrapper; the session carries
    /// the identity, so every prompt and workflow says plain `swamp`.
    #[test]
    fn nothing_routes_swamp_through_a_wrapper() {
        let mut files = vec![repo().join("models/@swamp/software-factory/ecphory-factory.yaml")];
        for entry in std::fs::read_dir(repo().join("workflows")).unwrap() {
            files.push(entry.unwrap().path());
        }
        for path in files {
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(
                !text.contains("scripts/swamp") && !text.contains(".envrc"),
                "{} still routes swamp through the wrapper",
                path.display()
            );
        }
        assert!(!repo().join("scripts/swamp").exists());
        assert!(!repo().join(".envrc").exists());
    }

    /// With `tools: [claude]`, `swamp repo init`/`upgrade` writes
    /// `.claude/settings.local.json` with a hook that runs `swamp audit record`
    /// after every Bash call, under whatever identity that shell has. On
    /// 2026-10-19 it ran 5 times as the wrong login before it was removed.
    /// `tools: []` makes swamp write no Claude settings and no CLAUDE.md.
    /// RED-PROOF: moved here from the deleted factory_swamp_wrapper.rs and
    /// re-observed 2026-10-19: `.swamp.yaml` back to `tools: [claude]` fails
    /// it. Live, `swamp repo upgrade` in a throwaway dir wrote both files with
    /// that and neither without (observed when it was first added).
    #[test]
    fn swamp_is_enrolled_for_no_ai_tool() {
        let marker = std::fs::read_to_string(repo().join(".swamp.yaml")).expect("read .swamp.yaml");
        let lines: Vec<&str> = marker.lines().collect();
        assert!(
            lines.contains(&"tools: []"),
            ".swamp.yaml must enroll no AI tool:\n{marker}"
        );
        assert!(
            lines.contains(&"telemetryDisabled: true"),
            ".swamp.yaml must keep telemetry off:\n{marker}"
        );
    }

    /// swamp's validate checks neither of these: a workflow the factory names,
    /// or a model a workflow step runs, can be missing and the definition still
    /// validates until a run reaches it.
    #[test]
    fn every_named_workflow_and_model_exists() {
        let mut named: Vec<String> = stages()
            .iter()
            .filter_map(|s| s["work"]["workflow"]["name"].as_str().map(str::to_string))
            .collect();
        named.sort();
        named.dedup();
        assert_eq!(
            named,
            [
                "openspec-archive-verify",
                "openspec-plan-verify",
                "openspec-tasks-verify",
                "pre-pr-verify"
            ]
        );
        for name in &named {
            let wf = load(&format!("workflows/workflow-{name}.yaml"));
            assert_eq!(wf["name"], name.as_str());
            for job in wf["jobs"].as_array().unwrap() {
                for step in job["steps"].as_array().unwrap() {
                    if let Some(model) = step["task"]["modelIdOrName"].as_str() {
                        let path = repo().join(format!("models/command/shell/{model}.yaml"));
                        assert!(
                            path.is_file(),
                            "workflow {name} step {} runs missing model {model}",
                            step["name"]
                        );
                    }
                }
            }
        }
    }
}
