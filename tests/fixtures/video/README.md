# Video Preview Fixtures

Stage 0 records the video cases the strict preview design must handle. Binary video samples are intentionally not stored in the repository yet.

Required categories:

- Progressive MP4 with metadata near the beginning.
- MP4 with `moov` metadata near the end.
- Large video that exceeds the default preview budget.
- Remote response that does not support `Range`.
- Remote 403 or expired dlink response.
- Redirecting dlink response.

The strict playback implementation must use synthetic HTTP fixtures first, then local read-only live smoke through the loopback proxy.

