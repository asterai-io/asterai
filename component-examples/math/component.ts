import { Output } from "./typegen/component";

export const add = (a: number, b: number): Output => {
  console.log("running component. a + b = ", a + b);
  return {
    value: a + b,
  };
};
