# Terminal segment 07c: the ttl-archival tests that prove the other two invariants fire on the
# buggy contract (violation lines) and that the fixed contract passes all three (ok lines).
# Run from the repository root: vhs scripts/video/ttl-tests.tape
# Requires compiled example tests (scripts/video/make-segments.sh builds them first).

Output breakwater-out/video/segments/seg07c.mp4

Set Shell "bash"
Set FontFamily "JetBrains Mono"
Set FontSize 26
Set Width 1920
Set Height 1080
Set Padding 48
Set Framerate 30
Set Theme "Dracula"
Set TypingSpeed 22ms
Set WindowBar Colorful
Set CursorBlink false

Hide
Type `export BASH_SILENCE_DEPRECATION_WARNING=1 PS1="$ "; clear`
Enter
Sleep 1s
Show

Sleep 600ms
Type "cargo test -p example-ttl-archival --test suite -- --nocapture --test-threads 1 2>&1 \"
Enter
Type "  | grep -E 'violation: .*(unbounded_ttl|restore_cost)|(fixed|guarded).* ok$'"
Sleep 500ms
Enter
Wait+Screen@120s /never_restores_anything \.\.\. ok/
Sleep 8s
