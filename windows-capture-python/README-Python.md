# Windows Capture &emsp; [![Licence]][Licence URL] [![Build Status]][repository] [![Latest Version]][pypi.org]

[Licence]: https://img.shields.io/crates/l/windows-capture
[Licence URL]: https://github.com/NiiightmareXD/windows-capture/blob/main/windows-capture-python/LICENCE

[Build Status]: https://img.shields.io/github/actions/workflow/status/NiiightmareXD/windows-capture/rust.yml
[repository]: https://github.com/NiiightmareXD/windows-capture/tree/main/windows-capture-python

[Latest Version]: https://img.shields.io/pypi/v/windows-capture
[pypi.org]: https://pypi.org/project/windows-capture

**Windows Capture** is a highly efficient Rust and Python library that enables you to capture the screen using the Graphics Capture API effortlessly. This library allows you to easily capture the screen of your Windows-based computer and use it for various purposes, such as creating instructional videos, taking screenshots, or recording your gameplay. With its intuitive interface and robust functionality, Windows Capture is an excellent choice for anyone looking for a reliable, easy-to-use screen-capturing solution.

**Note** this README.md is for [Python library](https://github.com/NiiightmareXD/windows-capture/tree/main/windows-capture-python) Rust library can be found [here](https://github.com/NiiightmareXD/windows-capture)  

## Features

- Only Updates The Frame When Required.
- High Performance.
- Easy To Use.
- Latest Screen Capturing API.

## Installation

Run this command

```
pip install windows-capture
```

## Usage

```python
from windows_capture import WindowsCapture, Frame, InternalCaptureControl

# Every Error From on_closed and on_frame_arrived Will End Up Here
capture = WindowsCapture(
    cursor_capture=None,
    draw_border=None,
    monitor_index=None,
    window_name=None,
)


# Called Every Time A New Frame Is Available
@capture.event
def on_frame_arrived(frame: Frame, capture_control: InternalCaptureControl):
    print("New Frame Arrived")

    # Save The Frame As An Image To The Specified Path
    frame.save_as_image("image.png")

    # Gracefully Stop The Capture Thread
    capture_control.stop()


# Called When The Capture Item Closes Usually When The Window Closes, Capture
# Session Will End After This Function Ends
@capture.event
def on_closed():
    print("Capture Session Closed")


capture.start()
```

## Benchmark

Windows Capture Is The Fastest Python Screen Capture Library

![Benchmark Showing Windows Capture Is The Fastest Python Screen Capture Library](https://github.com/NiiightmareXD/windows-capture/assets/90005793/444fa93e-5e27-48c8-8eb6-b9e21ab26452)

## Contributing

Contributions are welcome! If you find a bug or want to add new features to the library, please open an issue or submit a pull request.

## License

This project is licensed under the [MIT License](LICENSE).
