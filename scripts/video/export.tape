# Terminal segment 06: export the shrunk policy-bypass sequence as a Rust test, show what the
# test says it does, and run the committed copy of it.
# Run from the repository root: vhs scripts/video/export.tape
# Requires a built CLI and compiled example tests (scripts/video/make-segments.sh does both).

Output breakwater-out/video/segments/seg06.mp4

Set Shell "bash"
Set FontFamily "JetBrains Mono"
Set FontSize 26
Set Width 1920
Set Height 1080
Set Padding 48
Set Framerate 30
Set Theme "Dracula"
Set TypingSpeed 28ms
Set WindowBar Colorful
Set CursorBlink false

# Neutral prompt; the export goes to /tmp/breakwater-demo; cargo's progress (stderr) is dropped.
Hide
Type `export BASH_SILENCE_DEPRECATION_WARNING=1 PS1="$ " PATH="$PWD/target/debug:$PATH"; mkdir -p /tmp/breakwater-demo; breakwater() { command breakwater "$@" 2>/dev/null; }; clear`
Enter
Sleep 1s
Show

Sleep 500ms
Type "breakwater export examples/policy-bypass --seed 7 --out /tmp/breakwater-demo/repro.rs"
Enter
Wait+Screen@60s /exported/
Sleep 800ms
Type "head -n 3 /tmp/breakwater-demo/repro.rs"
Enter
Sleep 2.5s
Type "cargo test -p example-policy-bypass --test repro_policy_bypass_7 2>/dev/null"
Enter
Wait+Screen@60s /test result: ok/
Sleep 4s
