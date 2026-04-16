import sys

# bare except (should trigger code-bare-except)
try:
    do_thing()
except:
    pass

# sys.exit outside __main__ guard (should trigger p4-sys-exit)
sys.exit(1)
