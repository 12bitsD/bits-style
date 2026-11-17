import unused from "./other.js";

let globalCounter = 0;

export function computeResult(first, second, third, fourth, fifth) {
    if(fifth) {
        return first;
    } else {
        return second + third + fourth;
    }
}
