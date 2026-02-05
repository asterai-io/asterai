import * as host from "asterai:host/api@1.0.0";

const greet = (name: string) => {
  console.log(`hello ${name}!`);
};

export const greeter = {
  greet
};
