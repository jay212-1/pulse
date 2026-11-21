Pulse Agent
===========

A small read-only helper for show computers (playback, backup, media
servers). Pulse asks it over the network how the computer is doing:
CPU, memory, free disk space, uptime, and whether the show programs run.
The agent never changes anything on the computer.

Start
-----

  macOS / Linux:  ./pulse-agent
  Windows:        pulse-agent.exe

Options:

  --port 7471       TCP port (default 7471)
  --bind 0.0.0.0    address to listen on (default: all interfaces)
  --version         print the version

The first start on macOS: right-click > Open (the program is not notarised),
and allow incoming connections when the firewall asks. On Windows allow it
in the firewall prompt for private networks.

In Pulse
--------

Add the computer with the preset "Show computer with Pulse Agent", type its
address and the programs that must run (for example "QLab, Ableton Live").
Pulse turns the device red when a program is not running or the agent does
not answer, and yellow when the disk runs full, the CPU is busy or memory
runs out.

Check by hand: open http://<address>:7471/status in a browser.

Start it automatically
----------------------

  macOS:   System Settings > General > Login Items > add pulse-agent
  Windows: put a shortcut to pulse-agent.exe into  shell:startup
  Linux:   a systemd user service, for example
           ~/.config/systemd/user/pulse-agent.service with
             [Service]
             ExecStart=%h/pulse-agent/pulse-agent
             Restart=always
             [Install]
             WantedBy=default.target
           then: systemctl --user enable --now pulse-agent
