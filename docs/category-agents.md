# Portfolio Coordinators

Right-click a portfolio heading and choose **Portfolio Coordinator…**, then **Enable
portfolio coordinator…** (or **Configure provider, authentication and charter…**).
Setup provides clickable provider and authentication choices. Select
**Edit executable and portfolio charter…** for an optional CLI path and the
portfolio objectives, then explicitly enable the portfolio coordinator.

For an existing Codex subscription, select **Codex** and **Existing CLI
subscription login**. Select **Connect / refresh available models**, choose
**Overview**, and choose the model and effort. Connecting reads account/model
metadata; it does not start inference. **Review portfolio now** starts a review;
**Message portfolio coordinator…** continues its persistent conversation.

Changing providers disables coordination until explicitly enabled again and
retains conversation history. API authentication is an explicit choice with API
billing; there is no subscription-to-API fallback. Custom adapters require an
explicit executable and custom authentication. Claude/custom providers can
require an exact model ID instead of a discovered model list.

The portfolio panel keeps stable bounds while status updates arrive and while
changing pages, so the controls stay under the pointer. The workspace has
Overview, Conversation, Findings and Proposed work pages.
Runtime observations and AI assessments remain separate. Reviews are on demand,
scoped to current portfolio repositories and structured agent status. Terminal
histories are excluded. Repository tools are read only and reject private paths,
symlinks and paths outside member repositories. Reads and search are bounded;
SSH members use the same scoped helper.

Proposals require approval of an exact task and project conversation. Supported
native Codex delivery queues work without typing into a terminal draft. Other
harnesses offer a manual copy path. Delivery does not imply completion; verified
acceptance is separate. Disabling cancels portfolio work and revokes future
repository access while retaining history and project processes. Deleted
portfolios retain a read-only archive accessible from the blank sidebar menu.

Implementation lives in `src/category_agent.rs`, `src/category_agent_files.rs`,
`src/category_agent_runtime.rs`, `src/category_agent_ui.rs` and
`scripts/category-provider.py`. State is private to the session/portfolio.
Configuration uses revision guards; review and handoff records persist across
frontend reconnects. Reconnecting does not automatically start a review.

The feature is still undergoing broader acceptance testing. The focused selection
fixture (`scripts/test-session-manager.py --category-agent-selection`) exercises
setup, provider/authentication changes, explicit enable, metadata connection,
model and effort selection without real inference. The full workflow fixture
(`--category-agent-workflow`) covers reviews, cancellation and handoffs with fake
providers. Real-provider review acceptance and broader event integration remain
outstanding; account metadata discovery and Codex read-only permission checks
have been exercised separately. No real portfolio is enabled automatically.

The broader Herdr parity goal remains paused; this feature is separate work.
