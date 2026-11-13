#!/usr/bin/env python3
"""Requeue hosted-runner allocation failures, preserving successful jobs/artifacts."""

import argparse
import json
import os
import subprocess
import time
from pathlib import Path

REPOSITORY = os.environ.get("GITHUB_REPOSITORY", "SeventyfourSam/codex")
WORKFLOW = ".github/workflows/custom-release.yml"
ACQUISITION_FAILURE = (
    "The job was not acquired by Runner of type hosted even after multiple attempts"
)
BUILD_ARTIFACTS = {
    "Build aarch64-apple-darwin": "custom-aarch64-apple-darwin",
    "Build x86_64-pc-windows-msvc": "custom-x86_64-pc-windows-msvc",
}
RELEASE_JOBS = {"prepare", "Publish GitHub Release", *BUILD_ARTIFACTS}


def api(path: str, method: str = "GET", body: dict | None = None):
    command = ["gh", "api", "--method", method, f"repos/{REPOSITORY}/{path}"]
    if body is not None:
        command.extend(["--input", "-"])
    output = subprocess.check_output(
        command, input=json.dumps(body) if body is not None else None,
        text=True, encoding="utf-8", timeout=60,
    )
    return json.loads(output) if output.strip() else None


def report(message: str) -> None:
    print(message)
    if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with Path(summary).open("a", encoding="utf-8") as output:
            output.write(message + "\n")


def allocation_failure(run_id: int, attempt: int) -> dict | None:
    data = api(f"actions/runs/{run_id}/attempts/{attempt}/jobs?per_page=100")
    jobs = data["jobs"]
    if data["total_count"] > len(jobs):
        raise ValueError("Unexpected number of release jobs; inspect manually")
    failed = [job for job in jobs if job["conclusion"] in {"failure", "cancelled", "timed_out"}]
    if not failed:
        return None
    for job in failed:
        if job["name"] not in RELEASE_JOBS or job["steps"]:
            report("Execution or validation failed; automatic resource retry does not apply.")
            return None
        check_id = int(job["check_run_url"].rsplit("/", 1)[1])
        annotations = api(f"check-runs/{check_id}/annotations?per_page=100")
        if not any(ACQUISITION_FAILURE in item["message"] for item in annotations):
            report("No hosted-runner acquisition failure; leaving this run for inspection.")
            return None
    required = {
        BUILD_ARTIFACTS[job["name"]]
        for job in jobs
        if job["name"] in BUILD_ARTIFACTS and job["conclusion"] == "success"
    }
    artifacts = api(f"actions/runs/{run_id}/artifacts?per_page=100")["artifacts"]
    available = {item["name"] for item in artifacts if not item["expired"]}
    if missing := required - available:
        report(f"Reusable artifacts are missing or expired: {sorted(missing)}. "
               "The next scheduled stable check can start a fresh build.")
        return None
    report(f"Runner allocation failed: {', '.join(job['name'] for job in failed)}. "
           f"Retaining successful artifacts: {', '.join(sorted(required)) or 'none yet'}.")
    return failed[0]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inspect-run", type=int, help="Inspect an existing run without requeueing")
    parser.add_argument("--attempt", type=int)
    args = parser.parse_args()
    if REPOSITORY != "SeventyfourSam/codex":
        raise ValueError("This helper is restricted to the custom release repository")
    resume = bool(os.environ.get("RETRY_RUN_ID"))
    run_id = args.inspect_run or int(os.environ["RETRY_RUN_ID"] if resume else os.environ["GITHUB_RUN_ID"])
    run = api(f"actions/runs/{run_id}")
    attempt = args.attempt or int(
        os.environ["RETRY_ATTEMPT"] if resume
        else os.environ.get("GITHUB_RUN_ATTEMPT", str(run["run_attempt"]))
    )
    if run["path"] != WORKFLOW or run["head_branch"] != "custom":
        raise ValueError("Expected a Custom release run on custom")
    if args.inspect_run:
        job = allocation_failure(run_id, attempt)
        report(f"Read-only inspection: {'would requeue ' + job['name'] if job else 'no resource retry'}.")
        return
    if resume:
        # The dispatching job must finish before its original run can be rerun.
        deadline = time.monotonic() + 120
        while int(run["run_attempt"]) <= attempt:
            if int(run["run_attempt"]) == attempt and run["status"] == "completed":
                break
            if time.monotonic() >= deadline:
                raise TimeoutError("Original run has not finished; inspect before retrying")
            time.sleep(5)
            run = api(f"actions/runs/{run_id}")
        if int(run["run_attempt"]) != attempt:
            report("A newer attempt already exists; ignoring this stale retry request.")
            return
        if run["conclusion"] != "failure":
            report("Original run succeeded or was cancelled; no retry requested.")
            return
    if attempt >= 51:
        report("GitHub's 50-rerun limit was reached. The next scheduled stable check "
               "can start a fresh run for an unfinished release.")
        return
    if not (job := allocation_failure(run_id, attempt)):
        return
    if resume:
        api(f"actions/jobs/{job['id']}/rerun", "POST")
        report(f"Requeued {job['name']} in {run['html_url']}. "
               "Successful jobs and artifacts remain available to the dependent publish job.")
    else:
        # workflow_dispatch is allowed with GITHUB_TOKEN and avoids workflow_run
        # recursion limits. Only failed jobs and their dependents rerun afterward.
        api("actions/workflows/custom-release.yml/dispatches", "POST", {
            "ref": "custom",
            "inputs": {"retry_run_id": str(run_id), "retry_attempt": str(attempt)},
        })
        report(f"Queued resource recovery for {run['html_url']} (attempt {attempt}).")


if __name__ == "__main__":
    main()
