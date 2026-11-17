import unused from "./other.js";

let global_counter = 0;

export function compute_result(first, second, third, fourth, fifth) {
	if(fifth) {
		return first;
	} else {
		return second + third + fourth;
	}
}
