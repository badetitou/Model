# Model — demo.
#
# Regenerate from the repository root:
#   cargo build --release -p model-factory --example build   # so the run below is instant
#   vhs docs/demo.tape
#
# The project shown is Structure's jpetstore-6 fixture (46 Java files, already
# indexed by scip-java); any directory works.

Output assets/demo.gif

Set Shell "bash"
Set FontFamily "FiraCode Nerd Font"
Set FontSize 16
Set Width 1280
Set Height 400
Set Padding 28
Set Theme {"name":"Model","black":"#0B1020","red":"#FF7A90","green":"#6EE7B7","yellow":"#F5D67B","blue":"#8FB8FF","purple":"#A78BFA","cyan":"#5EEAD4","white":"#E7ECF7","brightBlack":"#5A6480","brightRed":"#FFA3B3","brightGreen":"#9BF3CF","brightYellow":"#FFE9A3","brightBlue":"#B8D2FF","brightPurple":"#C9B8FF","brightCyan":"#A7F3E4","brightWhite":"#FFFFFF","foreground":"#E7ECF7","background":"#0B1020","cursor":"#6EE7B7","selection":"#2A3350"}
Set WindowBar "Colorful"
Set WindowBarSize 36
Set TypingSpeed 25ms
Set Framerate 24

Sleep 500ms
Type "cargo run --release --quiet -p model-factory --example build -- ../Structure/fixtures/jpetstore-6"
Sleep 250ms
Enter
Sleep 4s
