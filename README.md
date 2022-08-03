
# What is this?

A test program to reproduce a bug in Hetzner vSwitch where an IP address sometimes does not transfer to another machine in a reasonable time.

It starts a program on N machines that switches an IP address between the machines every 30 seconds (by default).
The machine that holds the IP broadcasts gratuitous ARPs every 0.5 seconds (by default) and listens for UDP pings.
The other machines send UDP pings to the IP.

# Problem

Sometimes, usually in the first 10 minutes, the IP gets stuck on a machine. That machine keeps receiving the pings, accroding to pcap, despite the IP being removed from its network interface.

# How to run

Workstation dependencies:

- Linux desktop
- Terminal program `konsole` or `xterm`
- [Ruby](https://www.ruby-lang.org/en/documentation/installation/)
- [Rust](https://www.rust-lang.org/tools/install)

Compile and view available flags:

    rustup target add x86_64-unknown-linux-musl
    cargo run --release --target x86_64-unknown-linux-musl -- --help

Install the Debian 11 base image on 2 or more machines with root access via ssh key.
Add the machines to a vSwitch. Give the vSwitc a public IP range (this might work with a private one too, but I've not tried it).

You don't need to set up the vswitch or anything else on the machines. This program does that.

Run the program like this:

    ./run-remote.rb \
      --ip-to-juggle $VALID_IP_FROM_VSWITCH_RANGE \
      --netmask $VSWITCH_IP_RANGE_NETMASK \
      --gateway $VSWITCH_GATEWAY \
      -- $MACHINE1_IP $MACHINE2_IP ...

This should start a new console window for each machine.

The bug manifests as follows:
If you see **more than a few** `Received ... -> ... (but IP not held!)` messages, then the IP has failed to switch over, despite the machine that took the IP sending gratuitous ARPs every 0.5 seconds. (It is normal to see 1-3 of these messages when the IP switchover happens - that's fine.)

Usually the bug happens within 10 minutes, but it can sometimes take longer.

You can now Ctrl+C all the terminals and investigate further. Often (but not always) the IP stays stuck for several minutes.
