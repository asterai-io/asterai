import * as asterai from "asterai:host/api@0.1.0";
import { BinaryOperationResult } from "example:math/plugin@0.1.0";

export const mul = (a: number, b: number): BinaryOperationResult => {
  const result = a + b;
  // Send the calculation result to the agent.
  asterai.sendResponseToAgent(`the result is ${result}`);
  // This result is not seen by the agent, but it can be consumed by
  // other plugins calling this function.
  return {
    value: result,
  };
};
