// Keep a scrolling list pinned to the bottom while new lines arrive, unless the reader has scrolled
// up. Only the reader's own scrolling changes that choice; our own scrolls never do. (The PyQt
// prototype lost the bottom when wrapped lines finished laying out after the scroll.)

const NEAR_BOTTOM_PX = 30;

export class FollowBottom {
  private following = true;
  private ours = false;
  private readonly el: HTMLElement;

  constructor(el: HTMLElement) {
    this.el = el;
    el.addEventListener('scroll', () => {
      if (this.ours) return;
      this.following = el.scrollTop + el.clientHeight >= el.scrollHeight - NEAR_BOTTOM_PX;
    });
    // Late layout (fonts, wrapping) changes the height without a scroll event from the reader.
    new ResizeObserver(() => this.settle()).observe(el);
  }

  get isFollowing(): boolean {
    return this.following;
  }

  /** Call after changing the content. */
  settle(): void {
    if (!this.following) return;
    this.ours = true;
    this.el.scrollTop = this.el.scrollHeight;
    requestAnimationFrame(() => {
      this.el.scrollTop = this.el.scrollHeight;
      this.ours = false;
    });
  }
}
