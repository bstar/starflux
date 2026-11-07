# Project categories

The sidebar groups projects into collapsible categories. Right-click blank space
in the sidebar and choose **Create category**, or use **More → Categories**.
The blank-space menu also offers Add project, Manage categories, Expand all and
Collapse all. Category headers have equal internal top/bottom padding.
Existing and new projects start in
**Uncategorized**. Each project belongs to one category.

Click a category header to expand or collapse it. Its menu button or right-click
menu provides rename, manual ordering, project details and deletion. Deleting a
category moves its projects into Uncategorized; it does not close or stop them.
Uncategorized is always last and cannot be renamed or deleted.

Right-click a project and choose **Move project to category**. The picker also
offers **Create category and move project**. Drag project rows within or between
groups to order them, or drop onto a collapsed header to append the project.
The entire category header accepts a drop and highlights with **Drop project
here** while a project is over it. Release the mouse to move the project; a
collapsed category stays collapsed.
You can also drop in the empty sidebar space below the last item, above **Add
project**, to append to the final category. An insertion marker shows the end
position in graphical mode.
Drag category headers to change their order. Activity never reorders the list.

There is padding below each category, before the next category header. Drop a
project into that space to place it after the category's last project. This
also works when reordering projects already in the same category. The space
shows **Drop at end of category** during a drag in graphical mode.

Category membership and ordering are saved with the session and shared between
its windows. Expansion is saved independently for each frontend. Concurrent
category edits are guarded: a stale edit asks you to reopen the menu.

## Activity summaries

Headers keep summarizing all assigned projects when collapsed, including projects
hidden with **Close project**. Their total and hidden counts remain visible.
The category menu lists those projects; selecting one reveals it in this window
without replacing its running terminal.

- **Working** counts projects with one or more working agents.
- **Attention** counts projects with failures, questions or approval requests.
- **Unknown** means observations are missing, stale or explicitly unknown.
- **Idle** means fresh observations show no work or attention needs.

Counts refer to projects, not agents. One project can be working while another
agent in that same project needs attention. These are activity summaries, not
completion percentages. Hidden projects continue to be observed asynchronously;
collapsing or organizing groups does not alter agents or App Space workers.

## Keyboard and compact views

Tab to the Projects panel. Up/Down traverse visible rows; Left collapses a group
or returns from a project to its header; Right expands it. Enter toggles the
focused header or selects a project. Menus expose category order controls.
Explicit project navigation expands its category and scrolls to the project.

Cells mode and the compact Projects picker provide the same management actions.
The cells summary abbreviates counts as `W` (working), `!` (attention) and `?`
(unknown). Add project remains at the bottom of the sidebar.
