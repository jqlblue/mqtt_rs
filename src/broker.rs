use std::rc::{Rc};
use std::cell::{RefCell};
use std::collections::HashMap;

//is same identity
fn is_same<T>(lhs: &T, rhs: &T) -> bool {
    lhs as *const T == rhs as *const T
}

//the code below is horrible
//borrow doesn't return T, it returns Ref<T>
//we need to dereference it to get to the T inside
//is_same takes borrow so reapply the ampersand
fn is_same_subscriber<T: Subscriber>(lhs: Rc<RefCell<T>>, rhs: Rc<RefCell<T>>) -> bool {
    let lhs = lhs.clone();
    let rhs = rhs.clone();
    let res = is_same(&*lhs.borrow(), &*rhs.borrow()); //needed to live long enough
    res
}

pub trait Subscriber {
    fn new_message(&mut self, bytes: &[u8]);
}

pub struct Broker<T: Subscriber> {
    tree: Node<T>,
    use_cache: bool,
    cache: HashMap<String, Vec<Rc<RefCell<T>>>>,
}

struct Node<T: Subscriber> {
    children: HashMap<String, Node<T>>,
    leaves: Vec<Subscription<T>>,
}

struct Subscription<T: Subscriber> {
    subscriber: Rc<RefCell<T>>,
    topic: String,
}

impl<T: Subscriber> Subscription<T> {
    fn new(subscriber: Rc<RefCell<T>>, topic: &str) -> Self {
        Subscription { subscriber: subscriber.clone(), topic: topic.to_string() }
    }
}

impl<T: Subscriber> Node<T> {
    fn new() -> Self {
        Node { children: HashMap::new(), leaves: vec![] }
    }

    fn add_subscription(&mut self, subscription: Subscription<T>) {
        self.leaves.push(subscription);
    }
}

impl<T: Subscriber> Broker<T> {
    pub fn new(use_cache: bool) -> Self {
        Broker { tree: Node::new(), use_cache: use_cache, cache: HashMap::new() }
    }

    pub fn subscribe(&mut self, subscriber: Rc<RefCell<T>>, topic: &str) {
        self.invalidate_cache();
        let sub_parts : Vec<&str> = topic.split("/").collect();
        Self::ensure_node_exists(&sub_parts, &mut self.tree);
        Self::add_subscription_to_node(&mut self.tree, subscriber.clone(), &sub_parts, topic);
    }

    pub fn unsubscribe_all(&mut self, subscriber: Rc<RefCell<T>>) {
        self.invalidate_cache();
        Self::unsubscribe_impl(&mut self.tree, subscriber.clone(), &[], false);
    }

    pub fn unsubscribe(&mut self, subscriber: Rc<RefCell<T>>, topics: &[&str]) {
        self.invalidate_cache();
        Self::unsubscribe_impl(&mut self.tree, subscriber.clone(), topics, true);
    }

    pub fn publish(&mut self, topic: &str, payload: &[u8]) {

        if self.use_cache {
            if let Some(subscribers) = self.cache.get(topic) {
                for subscriber in subscribers {
                    subscriber.borrow_mut().new_message(payload);
                }
                return;
            }
        }


        let pub_parts : Vec<&str> = topic.split("/").collect();
        Self::publish_impl(&self.tree, &pub_parts, &payload, topic, self.use_cache, &mut self.cache);
    }

    fn ensure_node_exists(sub_parts: &[&str], node: &mut Node<T>) {
        if sub_parts.len() == 0 {
            return;
        }

        let part = &sub_parts[0][..];
        if !node.children.contains_key(part) {
            node.children.insert(sub_parts[0].to_string(), Node::new());
        }

        Self::ensure_node_exists(&sub_parts[1..], node.children.get_mut(part)
                                 .expect(&format!("Could not get node at {}", &part)));
    }

    fn add_subscription_to_node(tree: &mut Node<T>, subscriber: Rc<RefCell<T>>, sub_parts: &[&str], topic: &str) {
        if sub_parts.len() < 1 {
            panic!("oops");
        }

        let part = &sub_parts[0][..];
        let node = tree.children.get_mut(part)
            .expect(&format!("Could not get node at {}", &part));
        let sub_parts = &sub_parts[1..];

        if sub_parts.len() == 0 {
            node.add_subscription(Subscription::new(subscriber.clone(), topic));
        } else {
            Self::add_subscription_to_node(node, subscriber, sub_parts, topic);
        }
    }

    fn publish_impl(tree: &Node<T>, pub_parts: &[&str], payload: &[u8], topic: &str, use_cache: bool, cache: &mut HashMap<String, Vec<Rc<RefCell<T>>>>) {
        if pub_parts.len() < 1 {
            return;
        }

        let part = &pub_parts[0][..];
        let pub_parts = &pub_parts[1..];

        for part in vec![part, "#", "+"] {

            if let Some(node) = tree.children.get(part) {
                if pub_parts.len() == 0 || part == "#" {
                    Self::publish_node(&node, payload, topic, use_cache, cache);
                }

                //so that "finance/#" matches "finance"
                if pub_parts.len() == 0 && node.children.contains_key("#") {
                    Self::publish_node(node.children.get("#")
                                       .expect(&format!("Could not get node at {}", &part)),
                                       payload, topic, use_cache, cache);
                }

                Self::publish_impl(&node, pub_parts, payload, topic, use_cache, cache);
            }
        }
    }

    fn publish_node(node: &Node<T>, payload: &[u8], topic: &str, use_cache: bool, cache: &mut HashMap<String, Vec<Rc<RefCell<T>>>>) {
        for subscription in &node.leaves {
            let subscriber = subscription.subscriber.clone();
            subscriber.borrow_mut().new_message(payload);
            if use_cache {
                if !cache.contains_key(topic) {
                    cache.insert(topic.to_string(), vec![]);
                }
                if let Some(subscribers) = cache.get_mut(topic) {
                    subscribers.push(subscriber.clone());
                }
            }
        }
    }

    fn unsubscribe_impl(tree: &mut Node<T>, subscriber: Rc<RefCell<T>>, topics: &[&str], check_topics: bool) {
        tree.leaves.retain(|s| {
            let is_same_subscriber = is_same_subscriber(s.subscriber.clone(), subscriber.clone());
            //I have no idea why t below is &&&str, I added a tripe deref cos the compiler told me to
            let is_same_topic = !check_topics || topics.into_iter().find(|t| ***t == s.topic).is_some();
            !is_same_subscriber || !is_same_topic
        });

        if tree.children.len() == 0 {
            return;
        }

        for (_, node) in tree.children.iter_mut() {
            Self::unsubscribe_impl(node, subscriber.clone(), topics, check_topics);
        }
    }

    fn invalidate_cache(&mut self) {
        if self.use_cache {
            self.cache = HashMap::new();
        }
    }
}

#[cfg(test)]
struct TestSubscriber {
    msgs: Vec<Vec<u8>>,
}

#[cfg(test)]
impl TestSubscriber {
    fn new() -> Self {
        TestSubscriber{msgs: vec![]}
    }
}

#[cfg(test)]
impl Subscriber for TestSubscriber {
    fn new_message(&mut self, bytes: &[u8]) {
        self.msgs.push(bytes.to_vec());
    }
}

#[test]
fn test_subscribe() {
    let mut broker = Broker::<TestSubscriber>::new(false);
    let sub_rc = Rc::new(RefCell::new(TestSubscriber::new()));
    let subscriber = sub_rc.clone();
    broker.publish("topics/foo", &[0, 1, 2]);
    assert_eq!(subscriber.borrow().msgs.len(), 0);

    broker.subscribe(subscriber.clone(), "topics/foo");
    broker.publish("topics/foo", &[0, 1, 9]); //should get this
    broker.publish("topics/bar", &[2, 4, 6]); //shouldn't get this
    assert_eq!(subscriber.borrow().msgs.len(), 1);
    assert_eq!(subscriber.borrow().msgs[0], &[0, 1, 9]);

    broker.subscribe(subscriber.clone(), "topics/bar");
    broker.publish("topics/foo", &[1, 3, 5, 7]);
    broker.publish("topics/bar", &[2, 4]);
    assert_eq!(subscriber.borrow().msgs.len(), 3);
    assert_eq!(subscriber.borrow().msgs[0], &[0, 1, 9]);
    assert_eq!(subscriber.borrow().msgs[1], &[1, 3, 5, 7]);
    assert_eq!(subscriber.borrow().msgs[2], &[2, 4]);
}


#[test]
fn test_unsubscribe_all() {
    let mut broker = Broker::<TestSubscriber>::new(false);
    let sub_rc = Rc::new(RefCell::new(TestSubscriber::new()));
    let subscriber = sub_rc.clone();

    broker.subscribe(subscriber.clone(), "topics/foo");
    broker.publish("topics/foo", &[0, 1, 9]); //should get this
    broker.publish("topics/bar", &[2, 4, 6]); //shouldn't get this
    assert_eq!(subscriber.borrow().msgs.len(), 1);
    assert_eq!(subscriber.borrow().msgs[0], &[0, 1, 9]);

    broker.unsubscribe_all(subscriber.clone());
    broker.publish("topics/foo", &[0, 1, 9]);
    broker.publish("topics/bar", &[2, 4]);
    broker.publish("topics/baz", &[2, 4, 7, 11]);

    //shouldn't have changed
    assert_eq!(subscriber.borrow().msgs.len(), 1);
    assert_eq!(subscriber.borrow().msgs[0], &[0, 1, 9]);
}

#[test]
fn test_unsubscribe_one() {
    let mut broker = Broker::<TestSubscriber>::new(false);
    let sub_rc = Rc::new(RefCell::new(TestSubscriber::new()));
    let subscriber = sub_rc.clone();

    broker.subscribe(subscriber.clone(), "topics/foo");
    broker.subscribe(subscriber.clone(), "topics/bar");
    broker.publish("topics/foo", &[0, 1, 9]); //should get this
    broker.publish("topics/bar", &[2, 4]); //should get this
    broker.publish("topics/baz", &[2, 4, 7, 11]); //shouldn't get this
    assert_eq!(subscriber.borrow().msgs.len(), 2);
    assert_eq!(subscriber.borrow().msgs[0], &[0, 1, 9]);
    assert_eq!(subscriber.borrow().msgs[1], &[2, 4]);

    broker.unsubscribe(subscriber.clone(), &["topics/foo"]);
    broker.publish("topics/foo", &[0, 1, 9]); //shouldn't get this
    broker.publish("topics/bar", &[2, 4]); //should get this
    broker.publish("topics/baz", &[2, 4, 7, 11]); //shouldn't get this

    assert_eq!(subscriber.borrow().msgs.len(), 3);
    assert_eq!(subscriber.borrow().msgs[0], &[0, 1, 9]);
    assert_eq!(subscriber.borrow().msgs[1], &[2, 4]);
    assert_eq!(subscriber.borrow().msgs[2], &[2, 4]);
}


#[cfg(test)]
fn test_matches(pub_topic: &str, sub_topic: &str) -> bool {
    let mut broker = Broker::<TestSubscriber>::new(false);
    let sub_rc = Rc::new(RefCell::new(TestSubscriber::new()));
    let subscriber = sub_rc.clone();

    broker.subscribe(subscriber.clone(), sub_topic);
    broker.publish(pub_topic, &[0, 1, 2]);
    let subscriber = subscriber.borrow();
    subscriber.msgs.len() == 1
}

#[test]
fn test_wildcards() {
    assert_eq!(test_matches("foo/bar/baz", "foo/bar/baz"), true);
    assert_eq!(test_matches("foo/bar", "foo/+"), true);
    assert_eq!(test_matches("foo/baz", "foo/+"), true);
    assert_eq!(test_matches("foo/bar/baz", "foo/+"), false);
    assert_eq!(test_matches("foo/bar", "foo/#"), true);
    assert_eq!(test_matches("foo/bar/baz", "foo/#"), true);
    assert_eq!(test_matches("foo/bar/baz/boo", "foo/#"), true);
    assert_eq!(test_matches("foo/bla/bar/baz/boo/bogadog", "foo/+/bar/baz/#"), true);
    assert_eq!(test_matches("finance", "finance/#"), true);
    assert_eq!(test_matches("finance", "finance#"), false);
    assert_eq!(test_matches("finance", "#"), true);
    assert_eq!(test_matches("finance/stock", "#"), true);
    assert_eq!(test_matches("finance/stock", "finance/stock/ibm"), false);
    assert_eq!(test_matches("topics/foo/bar", "topics/foo/#"), true);
    assert_eq!(test_matches("topics/bar/baz/boo", "topics/foo/#"), false);
}

#[test]
fn test_subscribe_wildcards() {
    let mut broker = Broker::<TestSubscriber>::new(false);
    let sub_rc1 = Rc::new(RefCell::new(TestSubscriber::new()));
    let sub_rc2 = Rc::new(RefCell::new(TestSubscriber::new()));
    let sub_rc3 = Rc::new(RefCell::new(TestSubscriber::new()));
    let sub_rc4 = Rc::new(RefCell::new(TestSubscriber::new()));

    let subscriber1 = sub_rc1.clone();
    let subscriber2 = sub_rc2.clone();
    let subscriber3 = sub_rc3.clone();
    let subscriber4 = sub_rc4.clone();

    broker.subscribe(subscriber1.clone(), "topics/foo/+");
    broker.publish("topics/foo/bar", &[3]);
    broker.publish("topics/bar/baz/boo", &[4]); //shouldn't get this one
    assert_eq!(subscriber1.borrow().msgs, vec![&[3]]);

    broker.subscribe(subscriber2.clone(), "topics/foo/#");
    broker.publish("topics/foo/bar", &[3]);
    broker.publish("topics/bar/baz/boo", &[4]); //shouldn't get this one
    assert_eq!(subscriber1.borrow().msgs, vec![&[3], &[3]]);
    assert_eq!(subscriber2.borrow().msgs, vec![&[3]]);

    broker.subscribe(subscriber3.clone(), "topics/+/bar");
    broker.subscribe(subscriber4.clone(), "topics/#");

    broker.publish("topics/foo/bar", &[3]);
    broker.publish("topics/bar/baz/boo", &[4]);
    broker.publish("topics/boo/bar/zoo", &[5]);
    broker.publish("topics/foo/bar/zoo", &[6]);
    broker.publish("topics/bbobobobo/bar", &[7]);

    assert_eq!(subscriber1.borrow().msgs, vec![&[3], &[3], &[3]]);
    assert_eq!(subscriber2.borrow().msgs, vec![&[3], &[3], &[6]]);
    assert_eq!(subscriber3.borrow().msgs, vec![&[3], &[7]]);
    assert_eq!(subscriber4.borrow().msgs, vec![&[3], &[4], &[5], &[6], &[7]]);
}
