# lights-out

Turns off component lighting and controls an MSI MPG CORELIQUID K360 cooler.

## Quiet cooling with a thermal override

`lights-out daemon` sends the CPU temperature to the cooler every second and
selects cooling automatically:

| Condition | Cooler mode |
| --- | --- |
| Normal operation below 80°C | Silent |
| CPU reaches 80°C | Game preset, immediately |
| CPU reaches 85°C | Fixed 100% on all cooler fans and the pump, immediately |
| In Full, CPU stays below 80°C for 10 seconds | Step down to Game |
| In Game, CPU stays below 75°C for 15 seconds | Return to Silent |
| CPU sensor unavailable or invalid | Fixed 100%, then exit for service restart |

Cooling steps down in stages after sustained recovery, with separate thresholds
for heating and cooling to avoid repeated noise changes. A new hot sample always
escalates immediately. Full speed no longer stays latched through ordinary
70–79°C workloads after a brief spike.
The defaults provide headroom below the Ryzen 9 7900X3D's 89°C operating limit;
they cannot compensate for a failed pump, blocked airflow or poor cooler contact.

The daemon leaves the cooler at fixed full speed on graceful shutdown. The
systemd unit also applies full speed after a crash or forced stop. The cooler
needs fresh CPU temperatures for its preset curves; a stopped process must not
leave it using a stale cool sample. USB communication failures may prevent any
software override from reaching the hardware.

`lights-out off` only changes lighting. It no longer forces Silent mode over an
active thermal override. The daemon periodically reapplies its selected mode,
so manual fan settings are temporary while the daemon runs.

Read actual RPM, duty and configuration with `sudo lights-out status`. Mode IDs
are 0 (Silent), 2 (Game), and 3 (custom, used here for fixed 100%). This readback is
useful for checking that commands reached the cooler; temperature and physical
cooling performance still need to be checked under the user's normal workload.

## Build and service

```sh
nix develop --command cargo test --manifest-path lights-out/Cargo.toml -j 2
nix build .#default --cores 2 --max-jobs 1
```

The example systemd units use the source checkout's release binary. NixOS should
instead point both `ExecStart` and `ExecStopPost` to the built package, with
`After=lights-out.service`, `Restart=on-failure`, and `RestartSec=2`.

The MSI packet formats and channel order follow the
[liquidctl MSI driver](https://github.com/liquidctl/liquidctl/blob/main/liquidctl/driver/msi.py)
and [protocol documentation](https://github.com/liquidctl/liquidctl/blob/main/docs/developer/protocol/coreliquid.md).
