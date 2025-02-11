## About

An implementation of the [Kullback Test](https://corgi.rip/blog/kullback-in-ctf/) in Rust, compiled to
WebAssembly to work in the browser.

![The compiled program running](./test.png)

## Usage

You can play around with the compiled version [here](https://corgi.rip/kullback). 

To compile it yourself, install [wasm-pack](https://github.com/rustwasm/wasm-pack)
then run `wasm-pack build --target web`. Your output will be in the `pkg/` directory
which `test.html` can then load and use.

## License

Licensed under the [MIT license](http://opensource.org/licenses/MIT).
