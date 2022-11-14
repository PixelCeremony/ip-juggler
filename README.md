
# What is this?

A test program to reproduce a bug in Hetzner vSwitch where an IP address sometimes does not reroute to another machine in a reasonable time, often for several minutes.

The program run on N machines and switches and cycles IP address to the next machines every 30 seconds (by default).
The machine that holds the IP broadcasts Gratuitous ARPs every 0.5 seconds (by default).
The other machines send UDP pings to the IP. If the wrong machine receives the pings, an error is printed.

# Problem

Sometimes, usually within the first 10 minutes of running this program, the IP gets stuck on a machine. That machine keeps receiving the pings, accroding to pcap, despite the IP being removed from its network interface.

# Hetzner's response

Unfortunately, Hetzner support stated that vSwitches are not meant to be a failover solution and refused to fix this bug.

# How to run

Workstation dependencies:

- Linux desktop
- Terminal program `konsole` or `xterm`
- [Ruby](https://www.ruby-lang.org/en/documentation/installation/)
- [Rust](https://www.rust-lang.org/tools/install)

Compile and view available flags:

    rustup target add x86_64-unknown-linux-musl
    cargo run --release --target x86_64-unknown-linux-musl -- --help

Now set up the test servers:
- Install the Debian 11 base image on 2 or more machines, with root access via ssh key.
- Check that the machines' clocks are roughly in sync.
- Add the machines to a vSwitch. Give the vSwitch a public IP range. (Private IPs might work too, but I've not tried it).

You don't need to set up the vSwitch or anything else on the machines. This program does that automatically.

Start the test program on the servers like this:

    ./run-remote.rb \
      --ip-to-juggle $SOME_VALID_IP_FROM_VSWITCH_IP_RANGE \
      --netmask $VSWITCH_IP_RANGE_NETMASK \
      --gateway $VSWITCH_GATEWAY \
      -- \
      $MACHINE1_IP \
      $MACHINE2_IP

This should start a new console window for each machine.

# How to interpret output

The bug manifests as follows:
If you see **more than a few** `Received ... -> ... (but IP not held!)` messages, then the IP has failed to switch over, despite the machine that took the IP sending gratuitous ARPs every 0.5 seconds. (It's OK to see about 1-3 of these messages when the IP switchover happens.)

Usually the bug happens within 10 minutes, but it can sometimes take longer.

You can now Ctrl+C all the terminals and investigate further. Often (but not always) the IP stays stuck for several minutes.
