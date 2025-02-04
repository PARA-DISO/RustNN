//! An easy to use neural network library written in Rust.
//!
//! # Description
//! nn is a [feedforward neural network ](http://en.wikipedia.org/wiki/Feedforward_neural_network)
//! library. The library
//! generates fully connected multi-layer artificial neural networks that
//! are trained via [backpropagation](http://en.wikipedia.org/wiki/Backpropagation).
//! Networks are trained using an incremental training mode.
//!
//! # XOR example
//!
//! This example creates a neural network with `2` nodes in the input layer,
//! a single hidden layer containing `3` nodes, and `1` node in the output layer.
//! The network is then trained on examples of the `XOR` function. All of the
//! methods called after `train(&examples)` are optional and are just used
//! to specify various options that dictate how the network should be trained.
//! When the `go()` method is called the network will begin training on the
//! given examples. See the documentation for the `NN` and `Trainer` structs
//! for more details.
//!
//! ```rust
//! use nn::{NN, HaltCondition};
//!
//! // create examples of the XOR function
//! // the network is trained on tuples of vectors where the first vector
//! // is the inputs and the second vector is the expected outputs
//! let examples = [
//!     (vec![0f64, 0f64], vec![0f64]),
//!     (vec![0f64, 1f64], vec![1f64]),
//!     (vec![1f64, 0f64], vec![1f64]),
//!     (vec![1f64, 1f64], vec![0f64]),
//! ];
//!
//! // create a new neural network by passing a pointer to an array
//! // that specifies the number of layers and the number of nodes in each layer
//! // in this case we have an input layer with 2 nodes, one hidden layer
//! // with 3 nodes and the output layer has 1 node
//! let mut net = NN::new(&[2, 3, 1]);
//!
//! // train the network on the examples of the XOR function
//! // all methods seen here are optional except go() which must be called to begin training
//! // see the documentation for the Trainer struct for more info on what each method does
//! net.train(&examples)
//!     .halt_condition( HaltCondition::Epochs(10000) )
//!     .log_interval( Some(100) )
//!     .momentum( 0.1 )
//!     .rate( 0.3 )
//!     .go();
//!
//! // evaluate the network to see if it learned the XOR function
//! for &(ref inputs, ref outputs) in examples.iter() {
//!     let results = net.run(inputs);
//!     let (result, key) = (results[0].round(), outputs[0]);
//!     assert!(result == key);
//! }
//! ```

// use rand;
use rand::Rng;
use serde::{Deserialize, Serialize};
// use serde_json::Result;
use std::iter::{Enumerate, Zip};
use std::slice;
use time::{Duration, Instant};
use HaltCondition::{Epochs, Timer, MSE};
use LearningMode::Incremental;

static DEFAULT_LEARNING_RATE: f64 = 0.3f64;
static DEFAULT_MOMENTUM: f64 = 0f64;
static DEFAULT_EPOCHS: u32 = 1000;

/// Specifies when to stop training the network
#[derive(Debug, Copy, Clone)]
pub enum HaltCondition {
    /// Stop training after a certain number of epochs
    Epochs(u32),
    /// Train until a certain error rate is achieved
    MSE(f64),
    /// Train for some fixed amount of time and then halt
    Timer(Duration),
}

/// Specifies which [learning mode](http://en.wikipedia.org/wiki/Backpropagation#Modes_of_learning) to use when training the network
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LearningMode {
    /// train the network Incrementally (updates weights after each example)
    Incremental,
}

/// Used to specify options that dictate how a network will be trained
#[derive(Debug)]
pub struct Trainer<'a, 'b> {
    examples: &'b [(Vec<f64>, Vec<f64>)],
    rate: f64,
    momentum: f64,
    log_interval: Option<u32>,
    halt_condition: HaltCondition,
    learning_mode: LearningMode,
    nn: &'a mut NN,
}

/// `Trainer` is used to chain together options that specify how to train a network.
/// All of the options are optional because the `Trainer` struct
/// has default values built in for each option. The `go()` method must
/// be called however or the network will not be trained.
impl<'a, 'b> Trainer<'a, 'b> {
    /// Specifies the learning rate to be used when training (default is `0.3`)
    /// This is the step size that is used in the backpropagation algorithm.
    pub fn rate(&mut self, rate: f64) -> &mut Trainer<'a, 'b> {
        if rate <= 0f64 {
            panic!("the learning rate must be a positive number");
        }

        self.rate = rate;
        self
    }

    /// Specifies the momentum to be used when training (default is `0.0`)
    pub fn momentum(&mut self, momentum: f64) -> &mut Trainer<'a, 'b> {
        if momentum <= 0f64 {
            panic!("momentum must be positive");
        }

        self.momentum = momentum;
        self
    }

    /// Specifies how often (measured in batches) to log the current error rate (mean squared error) during training.
    /// `Some(x)` means log after every `x` batches and `None` means never log
    pub fn log_interval(&mut self, log_interval: Option<u32>) -> &mut Trainer<'a, 'b> {
        match log_interval {
            Some(interval) if interval < 1 => {
                panic!("log interval must be Some positive number or None")
            }
            _ => (),
        }

        self.log_interval = log_interval;
        self
    }

    /// Specifies when to stop training. `Epochs(x)` will stop the training after
    /// `x` epochs (one epoch is one loop through all of the training examples)
    /// while `MSE(e)` will stop the training when the error rate
    /// is at or below `e`. `Timer(d)` will halt after the [duration](https://doc.rust-lang.org/time/time/struct.Duration.html) `d` has
    /// elapsed.
    pub fn halt_condition(&mut self, halt_condition: HaltCondition) -> &mut Trainer<'a, 'b> {
        match halt_condition {
            Epochs(epochs) if epochs < 1 => {
                panic!("must train for at least one epoch")
            }
            MSE(mse) if mse <= 0f64 => {
                panic!("MSE must be greater than 0")
            }
            _ => (),
        }

        self.halt_condition = halt_condition;
        self
    }
    /// Specifies what [mode](http://en.wikipedia.org/wiki/Backpropagation#Modes_of_learning) to train the network in.
    /// `Incremental` means update the weights in the network after every example.
    pub fn learning_mode(&mut self, learning_mode: LearningMode) -> &mut Trainer<'a, 'b> {
        self.learning_mode = learning_mode;
        self
    }

    /// When `go` is called, the network will begin training based on the
    /// options specified. If `go` does not get called, the network will not
    /// get trained!
    pub fn go(&mut self) -> f64 {
        self.nn.train_details(
            self.examples,
            self.rate,
            self.momentum,
            self.log_interval,
            self.halt_condition,
        )
    }
}
/// Neural network
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NN {
    layers: Vec<Vec<Vec<f64>>>,
    num_inputs: u32,
}

impl NN {
    /// Each number in the `layers_sizes` parameter specifies a
    /// layer in the network. The number itself is the number of nodes in that
    /// layer. The first number is the input layer, the last
    /// number is the output layer, and all numbers between the first and
    /// last are hidden layers. There must be at least two layers in the network.
    pub fn new(layers_sizes: &[u32]) -> NN {
        let mut rng = rand::thread_rng();

        if layers_sizes.len() < 2 {
            panic!("must have at least two layers");
        }

        for &layer_size in layers_sizes.iter() {
            if layer_size < 1 {
                panic!("can't have any empty layers");
            }
        }

        let mut layers = Vec::with_capacity(layers_sizes.len());
        let mut it = layers_sizes.iter();
        // get the first layer size
        let first_layer_size = *it.next().unwrap();

        // setup the rest of the layers
        let mut prev_layer_size = first_layer_size;
        let sample = rand::distributions::Uniform::from(-0.5f64..0.5f64);
        it.for_each(|layer_size| {
            let layer = (0..*layer_size)
                .map(|_| {
                    (&mut rng)
                        .sample_iter(sample)
                        .take(prev_layer_size as usize + 1)
                        .collect::<Vec<f64>>()
                })
                .collect();
            layers.push(layer);
            prev_layer_size = *layer_size;
        });
        layers.shrink_to_fit();
        NN {
            layers: layers,
            num_inputs: first_layer_size,
        }
    }

    /// Runs the network on an input and returns a vector of the results.
    /// The number of `f64`s in the input must be the same
    /// as the number of input nodes in the network. The length of the results
    /// vector will be the number of nodes in the output layer of the network.
    pub fn run(&self, inputs: &[f64]) -> Vec<f64> {
        if inputs.len() as u32 != self.num_inputs {
            panic!("input has a different length than the network's input layer");
        }
        self.do_run(inputs).pop().unwrap()
    }

    /// Takes in vector of examples and returns a `Trainer` struct that is used
    /// to specify options that dictate how the training should proceed.
    /// No actual training will occur until the `go()` method on the
    /// `Trainer` struct is called.
    pub fn train<'b>(&'b mut self, examples: &'b [(Vec<f64>, Vec<f64>)]) -> Trainer {
        Trainer {
            examples: examples,
            rate: DEFAULT_LEARNING_RATE,
            momentum: DEFAULT_MOMENTUM,
            log_interval: None,
            halt_condition: Epochs(DEFAULT_EPOCHS),
            learning_mode: Incremental,
            nn: self,
        }
    }

    /// Encodes the network as a JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            // .ok()
            .expect("encoding JSON failed")
    }

    /// Builds a new network from a JSON string.
    pub fn from_json(encoded: &str) -> NN {
        serde_json::from_str::<NN>(encoded)
            // .ok()
            .expect("decoding JSON failed")
    }

    fn train_details(
        &mut self,
        examples: &[(Vec<f64>, Vec<f64>)],
        rate: f64,
        momentum: f64,
        log_interval: Option<u32>,
        halt_condition: HaltCondition,
    ) -> f64 {
        // check that input and output sizes are correct
        let input_layer_size = self.num_inputs;
        let output_layer_size = self.layers[self.layers.len() - 1].len();
        for &(ref inputs, ref outputs) in examples.iter() {
            if inputs.len() as u32 != input_layer_size {
                panic!("input has a different length than the network's input layer");
            }
            if outputs.len() != output_layer_size {
                panic!("output has a different length than the network's output layer");
            }
        }

        self.train_incremental(examples, rate, momentum, log_interval, halt_condition)
    }

    fn train_incremental(
        &mut self,
        examples: &[(Vec<f64>, Vec<f64>)],
        rate: f64,
        momentum: f64,
        log_interval: Option<u32>,
        halt_condition: HaltCondition,
    ) -> f64 {
        let mut prev_deltas = self.make_weights_tracker(0.0f64);
        let mut epochs = 0u32;
        let mut training_error_rate = 0f64;
        let start_time = Instant::now();
        let mut rap_time = Instant::now();
        // let mut rate = rate;
        println!("start training");
        loop {
            if epochs > 0 {
                // log error rate if necessary
                match log_interval {
                    Some(interval) if epochs % interval == 0 => {
                        let elp = rap_time.elapsed();
                        println!("error rate: {}", training_error_rate);
                        println!(
                            "rap time: {}.{} [sec]",
                            elp.whole_seconds(),
                            elp.subsec_milliseconds()
                        );
                        rap_time = Instant::now();
                    }
                    _ => (),
                }

                // check if we've met the halt condition yet
                match halt_condition {
                    Epochs(epochs_halt) => {
                        if epochs == epochs_halt {
                            break;
                        }
                    }
                    MSE(target_error) => {
                        if training_error_rate <= target_error {
                            break;
                        }
                    }
                    Timer(duration) => {
                        if start_time.elapsed() >= duration {
                            break;
                        }
                    }
                }
            }

            training_error_rate = 0f64;

            for &(ref inputs, ref targets) in examples.iter() {
                let results = self.do_run(inputs);
                let weight_updates = self.calculate_weight_updates(&results, targets);
                training_error_rate += calculate_error(&results, targets);
                self.update_weights(&weight_updates, &mut prev_deltas, rate, momentum)
            }

            epochs += 1;
        }

        training_error_rate
    }

    fn do_run(&self, inputs: &[f64]) -> Vec<Vec<f64>> {
        let mut results = Vec::with_capacity(self.layers.len() + 1);
        results.push(inputs.to_vec());
        for (layer_index, layer) in self.layers.iter().enumerate() {
            results.push(
                layer
                    .iter()
                    .map(|node| sigmoid(modified_dotprod(node, &results[layer_index])))
                    .collect::<Vec<_>>(),
            );
        }
        results
    }
    // updates all weights in the network
    fn update_weights(
        &mut self,
        network_weight_updates: &[Vec<Vec<f64>>],
        prev_deltas: &mut [Vec<Vec<f64>>],
        rate: f64,
        momentum: f64,
    ) {
        for layer_index in 0..self.layers.len() {
            unsafe {
                let layer = self.layers.get_unchecked_mut(layer_index);
                let layer_weight_updates = network_weight_updates.get_unchecked(layer_index);
                for node_index in 0..layer.len() {
                    let node = layer.get_unchecked_mut(node_index);
                    let node_weight_updates = layer_weight_updates.get_unchecked(node_index);
                    for (weight_index, node) in node.iter_mut().enumerate() {
                        let weight_update = *(node_weight_updates.get_unchecked(weight_index));
                        let prev_delta = *(prev_deltas
                            .get_unchecked(layer_index)
                            .get_unchecked(node_index)
                            .get_unchecked(weight_index));
                        let delta = (rate * weight_update) + (momentum * prev_delta);
                        *node += delta;
                        prev_deltas[layer_index][node_index][weight_index] = delta;
                    }
                }
            }
        }
    }

    // calculates all weight updates by backpropagation
    fn calculate_weight_updates(
        &self,
        results: &[Vec<f64>],
        targets: &[f64],
    ) -> Vec<Vec<Vec<f64>>> {
        let layer_num = (self.layers).len();
        let mut network_errors: Vec<Vec<f64>> = Vec::with_capacity(layer_num);
        let mut network_weight_updates = Vec::with_capacity(layer_num);
        let layers = &self.layers;
        let layer_num = layer_num - 1;
        let network_results = &results[1..]; // skip the input layer
        let mut next_layer_nodes: Option<&Vec<Vec<f64>>> = None;
        for (layer_index, (layer_nodes, layer_results)) in
            iter_zip_enum(layers, network_results).rev()
        {
            let prev_layer_results = &results[layer_index];
            let vec_len = layer_nodes.len();
            let mut layer_errors = Vec::with_capacity(vec_len);
            let mut layer_weight_updates = Vec::with_capacity(vec_len);
            for (node_index, (node, &result)) in iter_zip_enum(layer_nodes, layer_results) {
                let node_len = node.len();
                let mut node_weight_updates = Vec::with_capacity(node_len);
                // calculate error for this node
                let node_error = if layer_index == layer_num {
                    result * (1f64 - result) * (targets[node_index] - result)
                } else {
                    let next_layer_errors = &network_errors[network_errors.len() - 1];
                    let sum = next_layer_nodes
                        .unwrap()
                        .iter()
                        .zip((next_layer_errors).iter())
                        .fold(0f64, |acc, x| acc + x.0[node_index + 1] * x.1);
                    result * (1f64 - result) * sum
                };

                // calculate weight updates for this node
                for weight_index in 0..node_len{ unsafe {
                    let prev_layer_result = if weight_index == 0 {
                        1f64 // threshold
                    } else {
                        *prev_layer_results.get_unchecked(weight_index - 1)
                    };
                    let weight_update = node_error * prev_layer_result;
                    node_weight_updates.push(weight_update);
                }}

                layer_errors.push(node_error);
                layer_weight_updates.push(node_weight_updates);
            }

            network_errors.push(layer_errors);
            network_weight_updates.push(layer_weight_updates);
            next_layer_nodes = Some(layer_nodes);
        }

        // updates were built by backpropagation so reverse them
        network_weight_updates.reverse();

        network_weight_updates
    }

    fn make_weights_tracker<T: Clone>(&self, place_holder: T) -> Vec<Vec<Vec<T>>> {
        self.layers
            .iter()
            .map(|layer| {
                layer
                    .iter()
                    .map(|node| {
                        node.iter()
                            .map(|_| place_holder.clone())
                            .collect::<Vec<T>>()
                    })
                    .collect::<Vec<Vec<T>>>()
            })
            .collect::<Vec<_>>()
    }
}

fn modified_dotprod(node: &[f64], values: &[f64]) -> f64 {
    let mut it = node.iter();
    let tmp = *it.next().unwrap(); // start with the threshold weight
    it.zip(values.iter()).fold(tmp, |acc, x| x.0 * x.1 + acc)
}
#[inline(always)]
fn sigmoid(y: f64) -> f64 {
    (1f64 + (-y).exp()).recip()
}

// takes two arrays and enumerates the iterator produced by zipping each of
// their iterators together
#[inline(always)]
fn iter_zip_enum<'s, 't, S: 's, T: 't>(
    s: &'s [S],
    t: &'t [T],
) -> Enumerate<Zip<slice::Iter<'s, S>, slice::Iter<'t, T>>> {
    s.iter().zip(t.iter()).enumerate()
}

// calculates MSE of output layer
fn calculate_error(results: &Vec<Vec<f64>>, targets: &[f64]) -> f64 {
    let last_results = &results[results.len() - 1];
    last_results
        .iter()
        .zip(targets.iter())
        .fold(0f64, |acc, x| acc + (x.1 - x.0).powi(2))
        / (last_results.len() as f64)
}
