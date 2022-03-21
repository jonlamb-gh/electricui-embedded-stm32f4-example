# electricui-embedded-stm32f4-example

A very simple low level [RTIC](https://rtic.rs/1/book/en/) example for the [electricui-embedded crate](https://crates.io/crates/electricui-embedded).

## Run

```
cargo run --release

INFO - Starting
INFO - Initialized
INFO - << { DataLen(0), Type(8), Int(1), Offset(0), IdLen(1), Resp(1), Acknum(0) } Id(i)
INFO - >> { DataLen(2), Type(8), Int(1), Offset(0), IdLen(1), Resp(0), Acknum(0) }
INFO - << { DataLen(0), Type(8), Int(1), Offset(0), IdLen(1), Resp(1), Acknum(0) } Id(i)
INFO - >> { DataLen(2), Type(8), Int(1), Offset(0), IdLen(1), Resp(0), Acknum(0) }
INFO - << { DataLen(0), Type(0), Int(0), Offset(0), IdLen(4), Resp(1), Acknum(0) } Id(name)
INFO - >> { DataLen(8), Type(4), Int(0), Offset(0), IdLen(4), Resp(0), Acknum(0) }
INFO - << { DataLen(0), Type(0), Int(1), Offset(0), IdLen(1), Resp(0), Acknum(0) } Id(t)
INFO - >> { DataLen(38), Type(1), Int(1), Offset(0), IdLen(1), Resp(0), Acknum(0) }
INFO - >> { DataLen(1), Type(6), Int(1), Offset(0), IdLen(1), Resp(0), Acknum(0) }
INFO - << { DataLen(1), Type(6), Int(1), Offset(0), IdLen(1), Resp(1), Acknum(0) } Id(h)
INFO - >> { DataLen(1), Type(6), Int(1), Offset(0), IdLen(1), Resp(0), Acknum(0) }
INFO - << { DataLen(0), Type(0), Int(1), Offset(0), IdLen(1), Resp(0), Acknum(0) } Id(w)
INFO - >> { DataLen(1), Type(6), Int(0), Offset(0), IdLen(9), Resp(0), Acknum(0) }
INFO - >> { DataLen(1), Type(6), Int(0), Offset(0), IdLen(9), Resp(0), Acknum(0) }
INFO - >> { DataLen(2), Type(8), Int(0), Offset(0), IdLen(8), Resp(0), Acknum(0) }
INFO - >> { DataLen(8), Type(4), Int(0), Offset(0), IdLen(8), Resp(0), Acknum(0) }
INFO - << { DataLen(0), Type(6), Int(0), Offset(0), IdLen(9), Resp(1), Acknum(0) } Id(led_state)
INFO - >> { DataLen(1), Type(6), Int(0), Offset(0), IdLen(9), Resp(0), Acknum(0) }
INFO - << { DataLen(1), Type(6), Int(1), Offset(0), IdLen(1), Resp(1), Acknum(0) } Id(h)
INFO - >> { DataLen(1), Type(6), Int(1), Offset(0), IdLen(1), Resp(0), Acknum(0) }
INFO - << { DataLen(2), Type(8), Int(0), Offset(0), IdLen(8), Resp(1), Acknum(1) } Id(lit_time)
INFO - >> { DataLen(2), Type(8), Int(0), Offset(0), IdLen(8), Resp(0), Acknum(0) }
INFO - << { DataLen(0), Type(6), Int(0), Offset(0), IdLen(9), Resp(1), Acknum(0) } Id(led_state)
INFO - >> { DataLen(1), Type(6), Int(0), Offset(0), IdLen(9), Resp(0), Acknum(0) }
INFO - << { DataLen(1), Type(6), Int(1), Offset(0), IdLen(1), Resp(1), Acknum(0) } Id(h)
INFO - >> { DataLen(1), Type(6), Int(1), Offset(0), IdLen(1), Resp(0), Acknum(0) }
INFO - << { DataLen(0), Type(6), Int(0), Offset(0), IdLen(9), Resp(1), Acknum(0) } Id(led_state)
INFO - >> { DataLen(1), Type(6), Int(0), Offset(0), IdLen(9), Resp(0), Acknum(0) }
```

## Links

* [board](https://github.com/WeActTC/MiniSTM32F4x1)
* [pinout](https://raw.githubusercontent.com/WeActTC/MiniSTM32F4x1/master/images/STM32F4x1_PinoutDiagram_RichardBalint.png)
