# stream-dlp v0.3.7

A simpler, modern workspace with fewer interruptions.

- Charcoal navigation, indigo accents, clearer typography, and consistent cards across Download, My videos, and GIF converter.
- Settings replaces the About & tools popup, with sections for updates, tool status, and app information.
- Browser connection expands beneath the video link. Successful imports collapse into a session status; manual cookie import and the export guide remain available.
- Available updates appear quietly in the sidebar instead of opening a popup on startup.
- Installation uses a centered confirmation dialog. Cancel, Escape, or clicking outside dismisses it without downloading.
- GIF presets show the active selection; detailed controls live in Advanced settings.
- Native file and folder pickers remain unchanged. Download and update verification mechanisms are unchanged.

## Downloads

The Windows installer includes the download tools. The portable ZIP includes the app and helper setup script. Existing installations can discover this Public release through their update checker. No Mac package is included.

## Validation

33 enabled Rust tests pass; seven optional integration tests remain skipped. Windows UI checks covered the three main pages, inline browser connection, Settings, the noninterrupting update notice, and Escape-to-cancel installation confirmation. No update was installed during UI verification. Browser-session import behavior is covered by automated tests; real-profile import was not performed during this redesign.
