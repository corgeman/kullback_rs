mod utils;

use wasm_bindgen::prelude::*;
use plotters::prelude::*;
use plotters_canvas::CanvasBackend;
use web_sys::HtmlCanvasElement;
use base64::{Engine as _, prelude::BASE64_STANDARD as b64};
use base16ct::mixed::decode_vec as hexdec;
use std::collections::HashMap;

/*
TODO:

- If there is a consecutive chain of spikes (ex. periods 17,18,19 all 'spike'), then
  only place a diamond/text on the largest spike in that chain.

- Profiling shows that plotting is negligible,
  wasm.memset() and wasm.memcpy() both take ~25% of execution time. Find ways to reduce that?
*/

#[wasm_bindgen]
pub fn analyze(canvas: HtmlCanvasElement, name: &str, range: usize, fmt: &str, mut cache: Vec<f32>) -> Vec<f32> {
    utils::set_panic_hook();

    // If cache holds enough data to graph, use it!
    if range <= cache.len() {
        let cached = cache.clone().into_iter().take(range-1).collect();
        plot(canvas,cached);
        return cache 
    } // Otherwise we'll need to extend it down below.
   

    /*
    Decode the input from UTF8, hex, or base64.
    Kinda hacky since we're treating potentially binary data as characters,
    but we're only looking for a unique mapping here and UTF8 can map all byte values 0-255.

    Also, check if the data can be treated as bytes. We can use that to speed up the IoC calculation.
    */
    let (is_bytes, data) : (bool, Vec<char>) = match fmt {
        "UTF8" => (name.is_ascii() , name.chars().collect()),
        "HEX" => (true, hexdec(name).expect("Failed to decode hex")
                    .iter().map(|d| *d as char).collect()),
        "BASE64" => (true, b64.decode(name).expect("Failed to decode base64")
                        .iter().map(|d| *d as char).collect()),
        _ => panic!("Invalid format (!?)")
    };

    // Don't analyze extremely short data.
    assert!(data.len() >= 4, "Length of input is too short (4 chars minimum)");

    // Range cannot be greater than len(data)/2, because analyzing less than two full blocks is useless.
    // This will be limited client-side but I can't be 100% sure that works.
    assert!(range <= data.len()/2, "Range is too large, please decrease.");

    // Extend cache to hold everything we need
    cache.reserve(range-cache.len());

    // keeping for testing: ioc calculation with transpose()
    // for i in (cache.len()+1)..range {
    //     let transpositions = transpose(&data,i);
    //     cache.push(transpositions.iter()
    //     .map(|x| if is_bytes {fast_ioc(x)} else {slow_ioc(x)})
    //     .sum::<f32>() / transpositions.len() as f32);
    // }

    for i in (cache.len()+1)..range{
        let chunk_size = data.len().div_ceil(i);
        let transpositions = transpose3(&data,i);
        let cap = data.len() % i;

        let mut sum: f32 = 0.0;
        for (j, x) in transpositions.chunks_exact(chunk_size).enumerate() {
            let fixed = &x[..chunk_size-((j >= cap) as usize)];
            sum += if is_bytes {fast_ioc(fixed)} else {slow_ioc(fixed)}
        }
        cache.push(sum / ((transpositions.len()/chunk_size) as f32));
    }

    // Plot it!
    return plot(canvas,cache);
}

/// Plot a list of IoC results given an HTML canvas.
fn plot(canvas: HtmlCanvasElement, iocs: Vec<f32>) -> Vec<f32>{
    let range: usize = iocs.len()+1;

    // Calculate max/min/mean/standard deviation.
    let min = *iocs.iter().min_by(|x,y| x.total_cmp(y)).unwrap();
    let max = *iocs.iter().max_by(|x,y| x.total_cmp(y)).unwrap();
    let mean = iocs.iter().sum::<f32>() / iocs.len() as f32;
    let stdev = (iocs.iter().map(|x| (mean-x).powi(2)).sum::<f32>() / iocs.len() as f32).sqrt();

    // Find all IoC calculations 1.5 standard deviations from the mean
    let spikes: Vec<usize> = iocs.iter().enumerate().filter_map(|(i, x)| ((x-mean)/stdev > 1.5).then(|| i)).collect();

    // Begin plotting!
    let root = CanvasBackend::with_canvas_object(canvas)
    .expect("Failed to use canvas object")
    .into_drawing_area();
    root.fill(&WHITE).unwrap();

    let mut chart_builder = ChartBuilder::on(&root);
    chart_builder.margin(5).margin_top(15).set_left_and_bottom_label_area_size(35);
    let mut chart_context = chart_builder
        .build_cartesian_2d(0..range, (min*0.95)..(max*1.05))
        .expect("Failed to build chart context");
    chart_context.configure_mesh()
    .disable_mesh().draw().expect("Failed to setup chart axes"); // sounds redundant but required for axes

    let triangle = |x: usize, y: f32|{
        return EmptyElement::at((x, y))
            + TriangleMarker::new((0, 0), 7, ShapeStyle::from(&BLACK))
            + TriangleMarker::new((0, 0), 5, ShapeStyle::from(&RED))
    };

    let label = |x: usize, y: f32| {
        return EmptyElement::at((x, y))
            + Text::new(
                x.to_string(),
                (10, 0),
                ("sans-serif", 15.0).into_font(),
            )
    };

    chart_context.draw_series(LineSeries::new(
        (1..range).zip(iocs.clone().into_iter()),
        &BLUE,
    )).expect("Failed to plot line series");

    for i in spikes {
        chart_context.plotting_area().draw(&label(i+1, iocs[i])).expect("Failed to plot spike text");
        chart_context.plotting_area().draw(&triangle(i+1, iocs[i])).expect("Failed to plot triangle");
    }

    return iocs;
}

/// IOC calculator without hashmap usage, used if input is all 0-255.
fn fast_ioc(data: &[char]) -> f32 {
    let mut counts = [0usize; 256];
    let l = data.len() as f32;

    // frequency count
    data.iter().for_each(|x| counts[*x as usize] += 1);

    /*
    Calculate the IoC.
    This code is technically problematic because if x is 0, (x-1) will underflow.
    However, since that gets multiplied by x, the underflow ends up doing nothing.
    A filter_map() could easily fix this, but I can't read WebAssembly so I don't know 
    if it would impact speed.
    */
    counts.into_iter().map(|x| x*(x-1)).sum::<usize>() as f32 / (l*(l-1.0))
}

/// IOC calculator for text containing UTF8 characters.
fn slow_ioc(data: &[char]) -> f32 {
    let mut counts: HashMap<&char,usize> = HashMap::new();
    let l = data.len() as f32;

    for x in data {
        counts.entry(x).and_modify(|x| *x += 1).or_insert(1);
    }
    counts.into_values().map(|x| x*(x-1)).sum::<usize>() as f32 / (l*(l-1.0))
}


/*
This was my first attempt. Very ugly. Keeping to remember.
*/
/// Split a Vec into chunks of n size, then read those chunks by columns.
#[allow(dead_code)]
fn transpose<T: Copy>(data: &Vec<T>, n: usize) -> Box<[Vec<T>]>{
    let mut arr: Box<[Vec<T>]> = std::iter::repeat_with(|| 
        Vec::with_capacity(data.len().div_ceil(n)))
        .take(n)
        .collect(); 
    for (i, d) in data.iter().enumerate(){
        arr[i%n].push(*d);
    }
    arr
}

/*
Second attempt at writing transpose(). This one places the data all into a single Vec.
While better, it's still pretty gross because it causes data loss.
I don't use this because it (currently) causes some data loss. Specifically, the last data.len()%n characters get ignored.
This happened because I wasn't sure how to deal with them. You're supposed to call .chunks_exact() on the output of this function,
and each chunk is a single column of the transposition. That means each chunk has to the the same length.
So what should I do if the data is 'xyzxyzxyzxyzx' and n is 3? It would tranpose to this:
xxxxx
yyyy
zzzz
The first line is longer than the other two! That's no good. This function solves the problem by removing
the last 'x' from the input, making all lines equal-length. But again, data loss is not a good solution.

Alternatively, we could pad the last two lines to make them as long as the first, then have analyze()
strip off the padding, but what would we pad it with? 
An unmapped UTF8 codepoint? Return Vec<Option<char>> instead? Both of those felt *really* dumb, so I didn't do it.
*/
#[allow(dead_code)]
fn transpose2(data: &Vec<char>, n: usize) -> Vec<char> {
    let mut arr: Vec<char> = vec!['?'; data.len()-data.len()%n]; // (data.len()/n)*n) works but that looks dumb
    let chunk_size = data.len()/n;
    for (i, d) in data.iter().take(arr.len()).enumerate() {
        arr[((i*chunk_size)%(chunk_size*n))+(i/n)] = *d;
    }
    arr
}

/*
Solution number three to the transposition problem.
Continuing from transpose2(), it finally hit me that analyze() can already 
know which chunks will have padding or not-- it can calculate data.len() % n,
and that number is how many chunks will be padding-less. 

Therefore, we just pad with \0, and analyze() strips it off without even having
to look for it.
*/
fn transpose3(data: &Vec<char>, n: usize) -> Vec<char> {
    let chunk_size = data.len().div_ceil(n);
    let mut arr: Vec<char> = vec!['\u{0}'; chunk_size*n];
    for (i, d) in data.iter().enumerate() {
        arr[((i*chunk_size)%(chunk_size*n))+(i/n)] = *d;
    }
    arr
}