# lights-out

Turns off component lighting and controls an MSI MPG CORELIQUID K360 cooler.

## Two cooling modes

**Quiet is the default after each boot.** It keeps the radiator fans at a fixed
35%, waterblock fan at 25%, and pump at 70%. CPU power saving and a 3.6 GHz ceiling
reduce heat at its source. At 68°C the controller starts lowering that ceiling;
at 75°C it allows at most 1.2 GHz, and at 80°C at most 800 MHz. It slowly restores
speed after sustained cooling below 60°C. Normal workloads never select the
Game or Full fan presets in Quiet mode; performance gives way to low noise.

**Balanced** restores the CPU's full supported frequency range and normal
Balanced power profile. The cooler uses Silent normally, Game at 80°C, and
fixed Full at 85°C. Full steps down to Game after 10 continuous seconds below
80°C; Game returns to Silent after 15 seconds below 75°C.

Select **Quiet Cooling** or **Balanced Cooling** from the application menu,
or use:

```sh
sudo lights-out mode quiet
sudo lights-out mode balanced
lights-out mode                 # selected mode and last daemon report
sudo lights-out status          # actual fan/pump RPM, duty and configuration
```

Changes take effect on the daemon's next polling interval, without a restart.
The selection is stored in `/run/lights-out/mode`: it survives a service restart
and resets to Quiet on reboot. Install the package system-wide to expose the
commands and application-menu entries. The daemon must be running for selections
to take effect. It owns the CPU power profile and frequency limits; use these
mode controls instead of changing the desktop's power-profile slider separately.

### Safety boundary

Quiet holds fixed low cooler speeds in normal operation. An actual **85°C
emergency**, missing/invalid sensor, CPU-control failure, or daemon failure still
requests full cooling. Software cannot promise silence during a cooling fault
without risking another thermal shutdown. Quiet emergency recovery requires ten
seconds below 70°C before returning to the low fixed speeds. GPU and case fans
outside the MSI cooler are not controlled by these modes.

The daemon sends the real CPU temperature every second, verifies frequency
limits each cycle, and reasserts cooling/profile selection periodically. Profile
commands have bounded execution time. If CPU control fails, it attempts an
800 MHz ceiling and full cooling before systemd restarts it. Graceful shutdown
and `ExecStopPost` also leave the cooler at fixed Full, since a stopped daemon
cannot provide fresh temperature samples. USB failures can prevent commands
from reaching the cooler; software cannot compensate for failed hardware.

`lights-out off` only changes lighting. Manual low-level `fan` settings are
temporary while the daemon runs. Hardware mode IDs are 0 (Silent), 2 (Game),
and 3 (custom, used for both Quiet's fixed duties and emergency Full).

## Build and service

```sh
nix develop --command cargo test --manifest-path lights-out/Cargo.toml -j 2
nix build .#default --cores 2 --max-jobs 1
```

The example systemd units use the source checkout's release binary. NixOS should
instead point both `ExecStart` and `ExecStopPost` to the built package, with
`After=lights-out.service power-profiles-daemon.service`, `Restart=on-failure`,
`RestartSec=2`, `RuntimeDirectory=lights-out`, and
`RuntimeDirectoryPreserve=restart`. The packaged wrapper supplies
`powerprofilesctl` and `timeout` on PATH; source builds need those tools installed.

The MSI packet formats and channel order follow the
[liquidctl MSI driver](https://github.com/liquidctl/liquidctl/blob/main/liquidctl/driver/msi.py)
and [protocol documentation](https://github.com/liquidctl/liquidctl/blob/main/docs/developer/protocol/coreliquid.md).
