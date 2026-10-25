# Terminal segment 03: breakwater run examples/policy-bypass --seed 7
# Run from the repository root: vhs scripts/video/policy-bypass.tape
# Requires a built CLI (cargo build -p breakwater-cli) and compiled example tests,
# so that no compiler output appears; scripts/video/make-segments.sh does both.

Output breakwater-out/video/segments/seg03.mp4

Set Shell "bash"
Set FontFamily "JetBrains Mono"
Set FontSize 26
Set Width 1920
Set Height 1080
Set Padding 48
Set Framerate 30
Set Theme "Dracula"
Set TypingSpeed 55ms
Set WindowBar Colorful
Set CursorBlink false

# Neutral prompt and paths: no user name, no home directory, report under /tmp/breakwater-demo.
# Cargo's own progress goes to stderr and is dropped so only the CLI's output is shown.
Hide
Type `export BASH_SILENCE_DEPRECATION_WARNING=1 PS1="$ " TMPDIR=/tmp/breakwater-demo PATH="$PWD/target/debug:$PATH"; mkdir -p /tmp/breakwater-demo; breakwater() { command breakwater "$@" 2>/dev/null; }; clear`
Enter
Sleep 1s
Show

Sleep 800ms
Type "breakwater run examples/policy-bypass --seed 7"
Sleep 700ms
Enter
Wait+Screen@120s /reproduce with/
Sleep 9s
