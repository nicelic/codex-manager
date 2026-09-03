# code-Manager

code-Manager is a Windows local gateway and tool manager built with Go and Vue.

It provides a local management page for gateway settings, upstream connectivity, and related runtime tools. The application is packaged as a single Windows executable.

## Download and run

1. Open the repository's Releases page.
2. Download `code-Manager.exe` from the latest release.
3. Double-click the executable to start the local management service.

The management page opens in the default browser. By default, it is available at `http://127.0.0.1:7780`.

## Build from source

Run `build.bat` from the repository root. It rebuilds the frontend and creates the release executable at:

`releases\code-Manager\code-Manager.exe`

Development and release details are maintained in `README.txt`.
