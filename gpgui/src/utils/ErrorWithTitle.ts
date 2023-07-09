export default class ErrorWithTitle extends Error {
  public title: string;
  constructor(title: string, message: string) {
    super(message);
    this.title = title;
  }
}
