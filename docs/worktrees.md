# Git worktrees

Press **Ctrl+B**, release it, then **Shift+W** to open the repository's worktree picker. **Git worktrees…** is also in each project's right-click menu. Both graphical and cells presentations support the picker, forms and confirmation, including when Git and terminals run on an SSH project host.

The picker lists the primary checkout and linked worktrees together, with branches, paths and lock state. Select a worktree to open its project or inspect removal options. Missing/prunable entries are shown for diagnosis; this interface does not implicitly prune or repair them.

## Create and open

Choose **Create & open worktree…**, enter a new branch name and an optional path/base. A blank path creates a sibling of the primary checkout with a name derived from the repository and branch. A blank base uses HEAD. The parent directory must already exist; an existing destination is rejected. Creation uses `git worktree add -b` at a verified commit and never resets another checkout, copies uncommitted edits, stashes changes or overwrites an existing branch.

The new project stays on the source project's host and in its saved session. Its display name includes the repository project and branch. The worktree picker provides the repository grouping; the sidebar preserves its existing manual ordering. Each worktree gets its own terminal sessions and App Space state. Creation starts a fresh shell; it does not restart the repository's agents or reuse its running apps.

Opening an already-associated worktree in the same saved session reattaches its project and terminal processes. A second saved session gets its own project association. Existing projects in a linked worktree can be associated without changing their project identity or configured subdirectory. If that session has no primary-repository project, STAR/FLUX records one as the return destination; it does not start its shell until it is opened.

Only the requesting view switches on completion. If you navigate to another project while creation/opening runs, the worktree is saved and its status is reported without stealing your new selection. Other views discover the project without changing focus.

## Remove

Choose a linked worktree, then **Remove worktree…** and type **REMOVE**. Canceling keeps it intact. Removal retains the branch and committed work.

Removal checks the repository identity, host boot identity, directory device/inode, HEAD, branch, lock state and availability against the displayed reference. If any changed, refresh the picker and review the new reference. The primary/bare worktree and locked worktrees cannot be removed. Missing entries require explicit Git repair/pruning. A detached checkout whose commit is not retained by any named ref must first be given a branch/ref.

The checkout must be clean, including untracked **and ignored** files. Ignored build outputs can therefore block removal. STAR/FLUX does not force removal, delete branches, reset files, stash changes or recursively delete directories itself; Git performs its final checks too.

Terminate the worktree's terminals explicitly before removal. A live or uninspectable managed terminal blocks it. The host also checks accessible current-user process working directories, executables and open file descriptors beneath the worktree. Running apps/editors can block removal, and a recent App Space launch is treated as pending for 30 seconds. Close the using programs and retry; the removal action does not stop them for you.

During removal, managed projects cannot start new terminals or request an app launch. If Git/SSH returns an uncertain outcome, associations remain pending until a fresh host list establishes whether the original directory still exists. Reopening/refreshing can resolve a pending association; it never blindly repeats deletion. After successful removal, associated projects leave the catalog, their records remain on disk, and views showing them return to their repository project. Other projects and App Space workers are retained.

## Automation and metadata

The [automation API](automation.md) adds:

| Method | Parameters |
| --- | --- |
| `worktrees.list` | `project`; returns repository identity and opaque worktree references |
| `worktrees.create` | `project`, new `branch`, optional `path` and `base`; returns a `target` without opening a project |
| `worktrees.open` | `project`, complete `target`; opens/reattaches a project and returns its ID and origin |
| `worktrees.remove` | `project`, complete fresh `target`, `confirm: "REMOVE"` |

Writes use the API's durable operation UUID/retry rules. Pass targets through unchanged from a fresh list/create result; a stale target is rejected. Creation and opening are separate API operations, while the UI combines them explicitly.

Associations are private `PROJECT.worktree.json` files beside project metadata. They record the origin, host, target identity and active/pending/removed state. Repository operations are serialized by a host-side lock in the common Git directory, with controller-side association locks as well. Existing terminal/app startup paths remain unchanged for projects with no worktree association.
