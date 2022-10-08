# switch-pro-xy

A bluetooth proxy between Switch and Pro Controller. 

## Requirements

```
sudo apt install libdbus-1-dev
```

Disable plugins in BlueZ(This step may be performed automatically at runtime in the future, see [here](https://github.com/Brikwerk/nxbt/blob/master/nxbt/bluez.py#L96))


Add ` --compat --noplugin=*` after `ExecStart=/usr/lib/bluetooth/bluetoothd` in `/lib/systemd/system/bluetooth.service`

Run

```
sudo systemctl daemon-reload
sudo systemctl restart bluetooth
```

https://github.com/Brikwerk/nxbt/blob/master/nxbt/bluez.py#L96


## Usage

Get bluetooth address of Pro Controller and Switch

Replace [CONTROLLER_ADDR] and [SWITCH_ADDR] with your bluetooth addresses:

```
git clone https://github.com/spacemeowx2/switch-pro-xy.git
cd switch-pro-xy
cargo build
cargo run -- [CONTROLLER_ADDR] [SWITCH_ADDR]
```

* When the program starts, press and hold the small, circular
button on the back of the Pro Controller (near the USB-C input) until the
player lights begin Flashing.
* Once the program prints "Got Connection" and then "Waiting for Switch to
connect...", start up your Switch and navigate to the "Pair/Change Grip" menu.
* Switch should automatically connect to your PC.
* After a few seconds, when you see "About to start forwarding packets. Please
close the menu in 5s", close the Switch menu.
* When the program prints "Start forwarding packets", you are ready to go.

## Issues

* In Splatoon3, when you enter the Equip menu, the controller will be disconnected.

## Credits

* [nxbt](https://github.com/Brikwerk/nxbt)
