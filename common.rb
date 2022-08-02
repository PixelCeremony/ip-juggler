require 'shellwords'

def sh!(cmd)
    if cmd.is_a?(Array)
        system(Shellwords.join(cmd))
    elsif cmd.is_a?(String)
        system(cmd)
    else
        raise "Bad cmd: #{cmd.type}"
    end
    "Failed: #{cmd}: #{$?}" unless $?.success?
end

def sh_bg!(cmd)
    pid = Process.fork do
        sh!(cmd)
    end
    [cmd, pid]
end

def await_processes(commands_and_pids)
    commands_and_pids.each do |cmd, pid|
        Process.waitpid(pid)
        raise "Command failed: #{cmd}: #{$?}" unless $?.success?
    end
end

