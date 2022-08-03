#!/usr/bin/env ruby

require_relative './common'

usage = <<EOS
Usage: #{$0} <flags> -- <hosts>
Flags added automatically: --total-participants and --local-index
EOS
usage.strip!
usage = ""

flags = []
dashdash_seen = false
hosts = []
ARGV.each do |arg|
    if arg == '--'
        dashdash_seen = true
    elsif arg =~ /^-h|--help$/
        puts usage
        exit(0)
    elsif dashdash_seen
        hosts << arg
    else
        flags << arg
    end
end

if hosts.empty?
    puts usage
    exit(1)
end

puts "Building binary"
sh!("cargo build --release --target x86_64-unknown-linux-musl")

ssh_options = ['-o', 'StrictHostKeyChecking=no']

puts "Uploading binary"
await_processes(hosts.map do |ip|
    sh_bg!(['ssh', *ssh_options, "root@#{ip}", "killall", "-q", "ip-juggler", ";", "rm", "-f", "/root/ip-juggler"])
end)

await_processes(hosts.map do |ip|
    sh_bg!(['scp', *ssh_options, '-q', 'target/x86_64-unknown-linux-musl/release/ip-juggler', "root@#{ip}:/root/ip-juggler"])
end)

terminal_options = [
    ['konsole', '--separate', '--hold', '-e'],
    ['xterm', '-hold', '-e'],
    # Not tested:
    #['gnome-terminal', '--'],
    #['x-terminal-emulator', '-e'],
]
terminal = terminal_options.find { |t| `which #{t[0]}`.strip != '' }
raise "No terminal program found. Tried looking for: #{terminal_options}" if terminal == nil

puts "Running on all machines"
await_processes(hosts.each_with_index.map do |ip, i|
    sleep 0.2
    sh_bg!([
        *terminal,
        'ssh',
        *ssh_options,
        "root@#{ip}",
        '/root/ip-juggler',
        '--total-participants',
        hosts.size.to_s,
        '--local-index',
        i.to_s,
        *flags
    ])
end)
